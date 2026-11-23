use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::ntro;

/// The only header version of compiled Source 2 resources (`*_c` files).
const HEADER_VERSION: u16 = 12;
const BLOCK_ENTRY_LEN: usize = 12;

/// A compiled Source 2 resource (`.vmdl_c`, `.vmat_c`, `.vtex_c`, ...): a header and a table of
/// typed blocks (`RERL`, `REDI`/`RED2`, `NTRO`, `DATA`, `CTRL`, `MBUF`, `VBIB`, `ANIM`, `AGRP`,
/// `ASEQ`, `MRPH`, `PHYS`, ...). Borrows the file's bytes.
pub struct Resource<'a> {
    bytes: &'a [u8],
    file_size: u32,
    version: u16,
    blocks: Vec<Block>,
}

/// A block of a [`Resource`]: its four-character type and where its bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// The block's type (`DATA`, `CTRL`, ...).
    pub kind: [u8; 4],
    /// Where its bytes start in the file.
    pub offset: u32,
    /// How many bytes it has.
    pub size: u32,
}

/// A file named in a resource's `RERL` block (its external references).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRef {
    /// The reference's 64-bit id.
    pub id: u64,
    /// The referenced file's path.
    pub name: String,
}

impl<'a> Resource<'a> {
    /// Reads the header and block table of a compiled file; a table that points outside the file is an error.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes, "resource header");
        let file_size = c.u32()?;
        let header_version = c.u16()?;
        if header_version != HEADER_VERSION {
            return Err(malformed(
                "resource header",
                format!("header version {header_version}, not {HEADER_VERSION}"),
            ));
        }
        let version = c.u16()?;
        let table_at = c.offset32()?;
        let count = c.u32()? as usize;
        if count > bytes.len() / BLOCK_ENTRY_LEN {
            return Err(malformed("resource header", format!("{count} blocks")));
        }
        let mut c = Cursor::new(bytes, "resource block table");
        c.take(table_at)?;
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = c.array()?;
            // Offsets count from the offset field itself.
            let at = c.pos() as u64;
            let offset = at + u64::from(c.u32()?);
            let size = c.u32()?;
            if offset + u64::from(size) > bytes.len() as u64 {
                return Err(malformed(
                    "resource block table",
                    format!(
                        "block {} at {offset}+{size} past the end ({})",
                        kind_name(&kind),
                        bytes.len()
                    ),
                ));
            }
            blocks.push(Block {
                kind,
                offset: offset as u32,
                size,
            });
        }
        Ok(Self {
            bytes,
            file_size,
            version,
            blocks,
        })
    }

    /// The resource type's own version (the header's second field).
    pub fn version(&self) -> u16 {
        self.version
    }

    /// The size the header gives; textures and sounds keep streamed data after it.
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// The blocks in file order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The bytes of `block`.
    pub fn bytes(&self, block: &Block) -> &'a [u8] {
        let start = block.offset as usize;
        &self.bytes[start..start + block.size as usize]
    }

    /// The bytes of the first block of `kind`.
    pub fn block(&self, kind: &[u8; 4]) -> Option<&'a [u8]> {
        self.blocks
            .iter()
            .find(|b| &b.kind == kind)
            .map(|b| self.bytes(b))
    }

    /// The bytes after the header's file size (texture mips, sound samples).
    pub fn trailing(&self) -> &'a [u8] {
        self.bytes
            .get(self.file_size as usize..)
            .unwrap_or_default()
    }

    /// The first block of `kind`. `None` means the block is absent. A present block returns
    /// either its decoded KV3 document or the reason it could not be decoded.
    /// For a `DATA` block that can use either KV3 or NTRO, use [`Resource::data`].
    pub fn kv3(&self, kind: &[u8; 4]) -> Option<Result<kv3::Document>> {
        self.kv3_in(kind, &mut Budget::named("KV3"))
    }

    /// [`Resource::kv3`], decoded within `budget`.
    pub fn kv3_in(&self, kind: &[u8; 4], budget: &mut Budget) -> Option<Result<kv3::Document>> {
        let bytes = self.block(kind)?;
        Some(if kv3::is_kv3(bytes) {
            kv3::decode_in(bytes, budget)
        } else {
            Err(malformed(
                "resource KV3",
                format!("{} block has an unsupported encoding", kind_name(kind)),
            ))
        })
    }

    /// The `DATA` block as a tree: KV3, or the struct an `NTRO` block describes in older
    /// resources. `None` when `DATA` is plain binary (textures, sounds) or absent.
    pub fn data(&self) -> Option<Result<Value>> {
        self.data_in(&mut Budget::named("DATA"))
    }

    /// [`Resource::data`], decoded within `budget`.
    pub fn data_in(&self, budget: &mut Budget) -> Option<Result<Value>> {
        let data = self.blocks.iter().find(|b| &b.kind == b"DATA")?;
        let bytes = self.bytes(data);
        if kv3::is_kv3(bytes) {
            return Some(kv3::decode_in(bytes, budget).map(|doc| doc.root));
        }
        let ntro = self.blocks.iter().find(|b| &b.kind == b"NTRO")?;
        Some(self.external_refs_in(budget).and_then(|refs| {
            let names = refs.into_iter().map(|r| (r.id, r.name)).collect();
            ntro::read(
                self.bytes,
                ntro.offset as usize,
                data.offset as usize,
                &names,
                budget,
            )
        }))
    }

    /// The files this resource references (`RERL`), empty when it has none.
    pub fn external_refs(&self) -> Result<Vec<ExternalRef>> {
        self.external_refs_in(&mut Budget::named("RERL"))
    }

    /// [`Resource::external_refs`], charging copied names to `budget`.
    pub fn external_refs_in(&self, budget: &mut Budget) -> Result<Vec<ExternalRef>> {
        let Some(block) = self.blocks.iter().find(|b| &b.kind == b"RERL") else {
            return Ok(Vec::new());
        };
        // Offsets count from their own field and may point anywhere in the file, so names may
        // overlap: their copies are charged, or a small file would name gigabytes.
        let mut c = Cursor::new(self.bytes, "RERL");
        c.take(block.offset as usize)?;
        let list_at = c.offset32()?;
        let count = c.u32()? as usize;
        if count > block.size as usize / 16 {
            return Err(malformed("RERL", format!("{count} references")));
        }
        let mut c = Cursor::new(self.bytes, "RERL");
        c.take(list_at)?;
        budget.slots::<ExternalRef>(count)?;
        let mut refs = Vec::with_capacity(count);
        for _ in 0..count {
            let id = c.u64()?;
            let name_at = c.offset32()?;
            c.take(4)?;
            let mut name = Cursor::new(self.bytes, "RERL name");
            name.take(name_at)?;
            let name = budget.text(name.cstr()?)?;
            refs.push(ExternalRef { id, name });
        }
        Ok(refs)
    }
}

pub(crate) fn kind_name(kind: &[u8; 4]) -> String {
    String::from_utf8_lossy(kind).into_owned()
}
