//! Dota's localized tooltips and script values, read from KeyValues text in the install.
//!
//! [`TooltipCatalog`] reads abilities, items, hero facets and modifier text. Its
//! [`RichText`] spans represent emphasis, line breaks and placeholders. Use [`Context`]
//! for the current ability level, upgrades, talents and live values.
//!
//! [`tooltips`] provides the original raw item and ability fields and shop search aliases.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::budget::Budget;
use crate::error::Result;
use crate::keyvalues::{Kv, localization_table, parse_kv, parse_kv_in, text};

const LOCALIZATION: &str = "resource/localization/abilities_english.txt";
const PREFIX: &str = "dota_tooltip_ability_";
/// The suffix of the localization's name-form key: the neutral items carry their name only under
/// `DOTA_Tooltip_Ability_<name>:n`, while their other texts keep the plain keys.
const NAME_FORM: &str = ":n";
const VARIABLE: &str = "dota_ability_variable_";
/// The localization's search aliases of an item: `DOTA_SearchAlias_Ability_<name>`.
const ALIAS: &str = "dota_searchalias_ability_";
/// Most aliases kept of one name.
const MAX_ALIASES: usize = 16;

/// Whose texts to read: Dota names (`item_<name>` for items, the ability's name otherwise) and
/// the heroes whose ability scripts hold them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The items' and abilities' names.
    pub names: Vec<String>,
    /// The heroes (`pudge`) whose `npc_dota_hero_<hero>.txt` scripts are searched too.
    pub heroes: Vec<String>,
}

impl Wanted {
    /// The install files the texts come from: the localization, then the scripts in the order
    /// their blocks are searched. Read each from the package and give them to [`tooltips`].
    pub fn sources(&self) -> Vec<String> {
        let mut heroes: Vec<String> = self
            .heroes
            .iter()
            .map(|h| format!("scripts/npc/heroes/npc_dota_hero_{h}.txt"))
            .collect();
        heroes.sort();
        heroes.dedup();
        let mut out = vec![
            LOCALIZATION.to_owned(),
            "scripts/npc/items.txt".to_owned(),
            "scripts/npc/npc_abilities.txt".to_owned(),
        ];
        out.extend(heroes);
        out
    }
}

/// An item's or ability's tooltip, as Dota's localization and scripts give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tooltip {
    /// Its shown name.
    pub name: String,
    /// The description, when there is one.
    pub description: Option<String>,
    /// The lore line, when there is one.
    pub lore: Option<String>,
    /// The notes, in order.
    pub notes: Vec<String>,
    /// The other names the shop's search finds it by (at most 16).
    pub aliases: Vec<String>,
    /// The script's `AbilityValues`, each as the space-separated text of its levels, in script
    /// order.
    pub values: Vec<(String, String)>,
    /// `ItemCost`, from the script or its values.
    pub cost: Option<String>,
    /// `AbilityCooldown`, from the script or its values.
    pub cooldown: Option<String>,
    /// `AbilityManaCost`, from the script or its values.
    pub mana: Option<String>,
    /// `AbilityCastRange`, from the script or its values.
    pub range: Option<String>,
    /// The localization's label of each value that has one (`$variables` filled in), in
    /// the values' order.
    pub labels: Vec<(String, String)>,
}

/// The tooltips of `wanted`'s names that the localization has a name for (under the plain key, else
/// the name-form key), by name, from the localization and the scripts (the files of [`Wanted::sources`], in that order).
pub fn tooltips(
    wanted: &Wanted,
    localization: &[u8],
    scripts: &[&[u8]],
) -> Result<BTreeMap<String, Tooltip>> {
    let loc = localization_table(text(localization)?);
    let variables: HashMap<&str, &str> = loc
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(VARIABLE)?, v.as_str())))
        .collect();
    let mut found: HashMap<&str, &Kv> = HashMap::new();
    let trees = scripts
        .iter()
        .map(|s| parse_kv(text(s)?))
        .collect::<Result<Vec<_>>>()?;
    for tree in &trees {
        find_blocks(tree, &wanted.names, &mut found);
    }
    let mut out = BTreeMap::new();
    for name in &wanted.names {
        let plain = format!("{PREFIX}{name}");
        let Some(title) = loc
            .get(&plain)
            .or_else(|| loc.get(&format!("{plain}{NAME_FORM}")))
        else {
            continue;
        };
        let block = found.get(name.as_str()).copied();
        out.insert(name.clone(), entry(name, title, block, &loc, &variables));
    }
    Ok(out)
}

/// One name's tooltip.
fn entry(
    name: &str,
    title: &str,
    block: Option<&Kv>,
    loc: &HashMap<String, String>,
    variables: &HashMap<&str, &str>,
) -> Tooltip {
    let key = |suffix: &str| format!("{PREFIX}{name}{suffix}");
    let notes = (0..)
        .map_while(|n| loc.get(&key(&format!("_note{n}"))))
        .cloned()
        .collect();
    // The other names the shop's search finds it by, `;` between them.
    let aliases = loc
        .get(&format!("{ALIAS}{name}"))
        .map(|list| {
            list.split(';')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .take(MAX_ALIASES)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let values: Vec<(String, String)> = block
        .and_then(|b| b.get("AbilityValues"))
        .and_then(Kv::block)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), level_values(Some(v))?)))
        .collect();
    let field = |script_key: &str| {
        let own = level_values(block.and_then(|b| b.get(script_key))).filter(|s| !s.is_empty());
        own.or_else(|| {
            values
                .iter()
                .find(|(k, _)| k == script_key)
                .map(|(_, v)| v.clone())
        })
    };
    let labels = values
        .iter()
        .filter_map(|(k, _)| {
            let label = loc.get(&key(&format!("_{}", k.to_lowercase())))?;
            (!label.is_empty()).then(|| {
                (
                    k.clone(),
                    substitute(label, |name| variables.get(name).copied()),
                )
            })
        })
        .collect();
    Tooltip {
        name: title.to_owned(),
        description: loc.get(&key("_description")).cloned(),
        lore: loc.get(&key("_lore")).cloned(),
        notes,
        aliases,
        cost: field("ItemCost"),
        cooldown: field("AbilityCooldown"),
        mana: field("AbilityManaCost"),
        range: field("AbilityCastRange"),
        values,
        labels,
    }
}

/// A script value as the space-separated text Dota prints: a string, or a block's `value`.
fn level_values(v: Option<&Kv>) -> Option<String> {
    match v? {
        Kv::Text(s) => Some(s.trim().to_owned()),
        Kv::Block(_) => match v?.get("value")? {
            Kv::Text(s) => Some(s.trim().to_owned()),
            Kv::Block(_) => None,
        },
    }
}

/// `$name` replaced by the localization's variable of that name, where there is one.
fn substitute<'a>(label: &str, mut variable: impl FnMut(&str) -> Option<&'a str>) -> String {
    let mut out = String::new();
    let mut rest = label;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(tail.len());
        let word = &tail[..end];
        match variable(word) {
            Some(v) if !word.is_empty() => out.push_str(v),
            _ => {
                out.push('$');
                out.push_str(word);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The first block anywhere under `node` (depth first, in file order) for each wanted name
/// that holds an ability or item.
fn find_blocks<'a>(node: &'a Kv, wanted: &[String], found: &mut HashMap<&'a str, &'a Kv>) {
    let mut stack: Vec<(&'a str, &'a Kv)> = Vec::new();
    let push = |stack: &mut Vec<(&'a str, &'a Kv)>, node: &'a Kv| {
        stack.extend(
            node.block()
                .unwrap_or_default()
                .iter()
                .rev()
                .map(|(k, v)| (k.as_str(), v)),
        );
    };
    push(&mut stack, node);
    while let Some((k, v)) = stack.pop() {
        if v.block().is_none() {
            continue;
        }
        let holds = ["AbilityValues", "ItemCost", "AbilityBehavior"]
            .iter()
            .any(|key| v.get(key).is_some());
        if holds && !found.contains_key(k) && wanted.iter().any(|w| w == k) {
            found.insert(k, v);
        }
        push(&mut stack, v);
    }
}

/// How a localization variable's text is supplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableFormat {
    /// An ability or item value in a percent-delimited placeholder.
    Ability,
    /// A named or positional Panorama dialog variable.
    Dialog,
    /// An integer modifier property or dialog variable.
    Integer,
    /// A floating-point modifier property or dialog variable.
    Decimal,
}

/// A typed placeholder in localized tooltip text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    /// Script value, dialog variable, or modifier property name.
    pub name: String,
    /// The formatting requested by the template.
    pub format: VariableFormat,
}

/// Inline text styling from the localization file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextStyle {
    /// RGB font color from a hexadecimal color tag.
    pub color: Option<[u8; 3]>,
    /// Heading level, from 1 to 6.
    pub heading: Option<u8>,
    /// Bold emphasis.
    pub bold: bool,
    /// Italic emphasis.
    pub italic: bool,
    /// Panorama span classes, such as `GameplayValues GameplayVariable`.
    pub classes: Option<String>,
}

/// Text, a line break, or a variable in a localized template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateContent {
    /// Literal text.
    Text(String),
    /// A line break from markup or a localized newline.
    LineBreak,
    /// A placeholder for a static or runtime value.
    Variable(Variable),
}

/// One styled run in a tooltip template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSpan {
    /// Inline style at this point in the template.
    pub style: TextStyle,
    /// The content to display or resolve.
    pub content: TemplateContent,
}

/// Parsed tooltip localization. A HUD can render its spans without interpreting HTML.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichText {
    /// Spans in display order.
    pub spans: Vec<TemplateSpan>,
}

/// A rendered run of text. A newline denotes a line break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    /// Inline style.
    pub style: TextStyle,
    /// Display text.
    pub text: String,
}

/// Text ready to display, with diagnostics for any unavailable inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedText {
    /// Styled display runs.
    pub spans: Vec<StyledSpan>,
    /// Number of substituted placeholders.
    pub substitutions: usize,
    /// Unavailable variables in template order, once per occurrence.
    pub unresolved: Vec<Variable>,
}

impl RichText {
    /// Parses Source 2 tooltip markup and variable syntax.
    pub fn parse(template: &str) -> Self {
        let mut parsed = Self::default();
        let mut styles = Vec::new();
        let mut rest = template;
        while let Some(at) = rest.find(['<', '%', '{', '\n', '\r']) {
            parsed.push_text(&rest[..at], current_style(&styles));
            rest = &rest[at..];
            if let Some((tag, tail)) = markup(rest) {
                if update_style(tag, &mut styles) {
                    parsed.push(TemplateContent::LineBreak, current_style(&styles));
                }
                rest = tail;
            } else if let Some((variable, tail)) = placeholder(rest) {
                parsed.push(TemplateContent::Variable(variable), current_style(&styles));
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("%%") {
                parsed.push_text("%", current_style(&styles));
                rest = tail;
            } else if rest.starts_with(['\r', '\n']) {
                parsed.push(TemplateContent::LineBreak, current_style(&styles));
                rest = &rest[if rest.starts_with("\r\n") { 2 } else { 1 }..];
            } else {
                parsed.push_text(&rest[..1], current_style(&styles));
                rest = &rest[1..];
            }
        }
        parsed.push_text(rest, current_style(&styles));
        parsed
    }

    /// Renders a template from runtime values, for example a modifier's current properties.
    pub fn render_context(&self, context: &Context<'_>) -> RenderedText {
        self.render(|variable| context.runtime(variable))
    }

    /// Resolves placeholders through `lookup`. Unavailable values display as an em dash.
    pub fn render(&self, mut lookup: impl FnMut(&Variable) -> Option<String>) -> RenderedText {
        let mut output = RenderedText::default();
        for span in &self.spans {
            let text = match &span.content {
                TemplateContent::Text(text) => text.clone(),
                TemplateContent::LineBreak => "\n".to_owned(),
                TemplateContent::Variable(variable) => match lookup(variable) {
                    Some(text) => {
                        output.substitutions += 1;
                        text
                    }
                    None => {
                        output.unresolved.push(variable.clone());
                        "—".to_owned()
                    }
                },
            };
            output.spans.push(StyledSpan {
                style: span.style.clone(),
                text,
            });
        }
        output
    }

    fn push(&mut self, content: TemplateContent, style: Option<&TextStyle>) {
        self.spans.push(TemplateSpan {
            content,
            style: style.cloned().unwrap_or_default(),
        });
    }

    fn push_text(&mut self, text: &str, style: Option<&TextStyle>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && style.map_or_else(
                || last.style == TextStyle::default(),
                |style| *style == last.style,
            )
            && let TemplateContent::Text(previous) = &mut last.content
        {
            previous.push_str(text);
        } else {
            self.push(TemplateContent::Text(text.to_owned()), style);
        }
    }
}

fn current_style<'a>(styles: &'a [(&str, TextStyle)]) -> Option<&'a TextStyle> {
    styles.last().map(|(_, style)| style)
}

fn markup(rest: &str) -> Option<(&str, &str)> {
    let body = rest.strip_prefix('<')?;
    let end = body.find(['<', '>'])?;
    let tag = &body[..end];
    let name = tag.trim_start_matches('/').as_bytes().first()?;
    (body.as_bytes()[end] == b'>' && name.is_ascii_alphabetic()).then(|| (tag, &body[end + 1..]))
}

fn update_style<'a>(tag: &'a str, styles: &mut Vec<(&'a str, TextStyle)>) -> bool {
    let end = tag.find(|c: char| c.is_ascii_whitespace() || c == '/');
    if let Some(closing) = tag.strip_prefix('/') {
        if let Some(index) = styles
            .iter()
            .rposition(|(name, _)| name.eq_ignore_ascii_case(closing.trim()))
        {
            styles.truncate(index);
        }
        return false;
    }
    let name = &tag[..end.unwrap_or(tag.len())];
    if name.eq_ignore_ascii_case("br") {
        return true;
    }
    if styles.len() >= 64 || tag.ends_with('/') {
        return false;
    }
    let mut style = current_style(styles).cloned().unwrap_or_default();
    match name.to_ascii_lowercase().as_str() {
        "font" => {
            style.color = attribute(&tag[name.len()..], "color")
                .and_then(color)
                .or(style.color)
        }
        "span" => {
            if let Some(classes) = attribute(&tag[name.len()..], "class") {
                style.classes = Some(classes.to_owned());
            }
        }
        "b" | "strong" => style.bold = true,
        "i" | "em" => style.italic = true,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => style.heading = Some(name.as_bytes()[1] - b'0'),
        _ => return false,
    }
    styles.push((name, style));
    false
}

fn attribute<'a>(attributes: &'a str, key: &str) -> Option<&'a str> {
    let (name, value) = attributes.trim().split_once('=')?;
    if !name.trim().eq_ignore_ascii_case(key) {
        return None;
    }
    let value = value.trim_start();
    let end = match value.as_bytes().first()? {
        b'\'' => value[1..].split('\'').next()?,
        b'"' => value[1..].split('"').next()?,
        _ => value.split_ascii_whitespace().next()?,
    };
    (end.len() <= 128).then_some(end)
}

fn color(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..], 16).ok()?,
    ])
}

fn identifier(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(value.len());
    value.split_at(end)
}

fn placeholder(rest: &str) -> Option<(Variable, &str)> {
    if let Some(body) = rest.strip_prefix('%') {
        let (name, tail) = identifier(body);
        if name.is_empty() {
            return None;
        }
        let (name, format, tail) = match (name, tail.strip_prefix('%')) {
            (name, Some(tail)) if name.starts_with("dMODIFIER_PROPERTY_") => {
                (&name[1..], VariableFormat::Integer, tail)
            }
            (name, Some(tail)) if name.starts_with("fMODIFIER_PROPERTY_") => {
                (&name[1..], VariableFormat::Decimal, tail)
            }
            (name, Some(tail)) => (name, VariableFormat::Ability, tail),
            (name, None)
                if name.starts_with('s')
                    && name.as_bytes().get(1).is_some_and(u8::is_ascii_digit) =>
            {
                (&name[..2], VariableFormat::Dialog, &body[2..])
            }
            _ => return None,
        };
        return Some((
            Variable {
                name: name.to_owned(),
                format,
            },
            tail,
        ));
    }
    let (format, body) = rest.strip_prefix('{')?.split_at_checked(2)?;
    let format = match format {
        "s:" => VariableFormat::Dialog,
        "d:" => VariableFormat::Integer,
        "f:" => VariableFormat::Decimal,
        _ => return None,
    };
    let (name, tail) = identifier(body);
    (!name.is_empty()).then_some(())?;
    Some((
        Variable {
            name: name.to_owned(),
            format,
        },
        tail.strip_prefix('}')?,
    ))
}

/// Inputs used to resolve ability and item values. Supply runtime text for values derived
/// from the live game, including modifier properties and script-defined dynamic values.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    /// One-based ability level. `None` displays all available levels.
    pub level: Option<usize>,
    /// One-based hero level, for hero-level scaling.
    pub hero_level: Option<usize>,
    /// Named one-based levels, e.g. `("quaslevel", 4)` for an Invoker value's `levelkey`.
    pub named_levels: &'a [(&'a str, usize)],
    /// Active facet script name.
    pub facet: Option<&'a str>,
    /// Active Aghanim's Scepter upgrade.
    pub scepter: bool,
    /// Variant ID for abilities with different Scepter upgrades.
    pub scepter_variant: Option<u32>,
    /// Active Aghanim's Shard upgrade.
    pub shard: bool,
    /// Variant ID for abilities with different Shard upgrades.
    pub shard_variant: Option<u32>,
    /// Learned talent names and other active script bonus keys.
    pub bonuses: &'a [&'a str],
    /// Display values provided by the caller, looked up before script values.
    pub runtime: &'a [(&'a str, &'a str)],
}

impl Context<'_> {
    fn has_bonus(&self, key: &str) -> bool {
        match key {
            "special_bonus_scepter" => self.scepter,
            "special_bonus_shard" => self.shard,
            _ => {
                for (prefix, enabled, variant) in [
                    ("special_bonus_scepter_", self.scepter, self.scepter_variant),
                    ("special_bonus_shard_", self.shard, self.shard_variant),
                ] {
                    if let Some(suffix) = key.strip_prefix(prefix)
                        && enabled
                        && suffix
                            .parse::<u32>()
                            .ok()
                            .is_some_and(|id| Some(id) == variant)
                    {
                        return true;
                    }
                }
                key.strip_prefix("special_bonus_facet_")
                    .is_some_and(|facet| self.facet == Some(facet))
                    || self.bonuses.contains(&key)
            }
        }
    }

    fn runtime(&self, variable: &Variable) -> Option<String> {
        self.runtime
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&variable.name))
            .map(|(_, value)| (*value).to_owned())
    }
}

/// One numeric ability value or stat, with its level array and script metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScalingValue {
    /// Base values in level order. A scalar is used at every level.
    pub levels: Vec<f64>,
    /// Script metadata such as `display_type`, `levelkey`, `dynamic_value` and bonus expressions.
    pub attributes: Vec<(String, String)>,
    /// Conditional bonus blocks that have values and their own modifiers.
    pub conditional: BTreeMap<String, ScalingValue>,
}

impl ScalingValue {
    fn parse(node: &Kv) -> Self {
        let block = children(Some(node));
        Self {
            levels: levels(node.as_str().or_else(|| field(node, "value"))),
            conditional: block
                .iter()
                .filter(|(key, value)| key.starts_with("special_bonus_") && value.block().is_some())
                .map(|(key, value)| (key.clone(), Self::parse(value)))
                .collect(),
            attributes: block
                .iter()
                .filter_map(|(key, value)| {
                    (key != "value").then_some((key.clone(), value.as_str()?.to_owned()))
                })
                .collect(),
        }
    }

    /// The value's script metadata field, when present.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Resolves the selected level or the whole level array in the supplied context.
    /// Runtime-only values have no static array.
    pub fn resolve(&self, context: &Context<'_>) -> Vec<f64> {
        self.resolve_with(context, false)
    }

    fn resolve_with(&self, context: &Context<'_>, bonus_only: bool) -> Vec<f64> {
        if self.attribute("dynamic_value").is_some_and(truthy)
            || (self.attribute("RequiresShard").is_some_and(truthy) && !context.shard)
        {
            return Vec::new();
        }
        let bonuses = self.bonus_adjustments(context);
        let growth = self.hero_growth(context);
        if bonuses.is_empty() && (bonus_only || (self.levels.is_empty() && growth.is_none())) {
            return Vec::new();
        }
        let level = self.attribute("levelkey").map_or(context.level, |key| {
            context
                .named_levels
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| *value)
        });
        let length = bonuses
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(self.levels.len())
            .max(usize::from(growth.is_some()));
        let indices = level.map_or(0..length, |level| {
            let index = level.saturating_sub(1).min(length.saturating_sub(1));
            index..index + 1
        });
        indices
            .map(|index| {
                let base = at_level(&self.levels, index).copied().unwrap_or_default()
                    + growth.unwrap_or_default();
                if bonus_only {
                    bonuses
                        .iter()
                        .filter_map(|bonus| at_level(bonus, index))
                        .map(|adjustment| adjustment.amount)
                        .sum::<f64>()
                        .abs()
                } else {
                    bonuses
                        .iter()
                        .filter_map(|bonus| at_level(bonus, index))
                        .fold(base, |value, adjustment| adjustment.apply(value))
                }
            })
            .collect()
    }

    fn bonus_adjustments(&self, context: &Context<'_>) -> Vec<Vec<Adjustment>> {
        let scalar = self
            .attributes
            .iter()
            .filter(|(key, _)| key.starts_with("special_bonus_") && context.has_bonus(key))
            .map(|(_, value)| adjustments(value));
        let nested = self
            .conditional
            .iter()
            .filter(|(key, _)| context.has_bonus(key))
            .map(|(_, value)| {
                value
                    .resolve(context)
                    .into_iter()
                    .map(|amount| Adjustment {
                        operation: '+',
                        amount,
                    })
                    .collect()
            });
        scalar
            .chain(nested)
            .filter(|values| !values.is_empty())
            .collect()
    }

    fn hero_growth(&self, context: &Context<'_>) -> Option<f64> {
        let level = context.hero_level?;
        let interval = self
            .attribute("levelup_interval")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let growth = self.attribute("hero_levelup").and_then(finite)?;
        Some(growth * (level.saturating_sub(1) / interval) as f64)
    }
}

fn at_level<T>(levels: &[T], index: usize) -> Option<&T> {
    levels.get(index).or_else(|| levels.last())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Adjustment {
    operation: char,
    amount: f64,
}

impl Adjustment {
    fn apply(self, base: f64) -> f64 {
        match self.operation {
            '=' => self.amount,
            'x' => base * self.amount,
            '%' => base * (1.0 + self.amount / 100.0),
            _ => base + self.amount,
        }
    }
}

fn adjustments(raw: &str) -> Vec<Adjustment> {
    raw.split_ascii_whitespace()
        .map(|value| {
            let (operation, number) = match value.as_bytes().first()? {
                b'=' => ('=', &value[1..]),
                b'x' | b'*' => ('x', &value[1..]),
                _ => value
                    .strip_suffix('%')
                    .map_or(('+', value), |number| ('%', number)),
            };
            Some(Adjustment {
                operation,
                amount: finite(number)?,
            })
        })
        .collect::<Option<_>>()
        .unwrap_or_default()
}

fn finite(raw: &str) -> Option<f64> {
    let value = raw.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn levels(raw: Option<&str>) -> Vec<f64> {
    raw.unwrap_or_default()
        .split_ascii_whitespace()
        .map(finite)
        .collect::<Option<_>>()
        .unwrap_or_default()
}

fn children(node: Option<&Kv>) -> &[(String, Kv)] {
    node.and_then(Kv::block).unwrap_or_default()
}

fn field<'a>(node: &'a Kv, key: &str) -> Option<&'a str> {
    node.get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn flags(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn read_scaling(node: &Kv) -> BTreeMap<String, ScalingValue> {
    let mut values = BTreeMap::new();
    for (key, value) in children(Some(node)) {
        if key.starts_with("Ability")
            || matches!(
                key.as_str(),
                "ItemCost" | "MaxLevel" | "LevelsBetweenUpgrades" | "RequiredLevel"
            )
        {
            let value = ScalingValue::parse(value);
            if !value.levels.is_empty() {
                values.insert(key.to_ascii_lowercase(), value);
            }
        }
    }
    for (_, group) in children(node.get("AbilitySpecial")) {
        for (name, value) in children(Some(group)) {
            let value = ScalingValue::parse(value);
            if !value.levels.is_empty() && !name.starts_with("LinkedSpecialBonus") {
                values.insert(name.to_ascii_lowercase(), value);
            }
        }
    }
    for (key, value) in children(node.get("AbilityValues")) {
        let parsed = ScalingValue::parse(value);
        let key = key.to_ascii_lowercase();
        if parsed.levels.is_empty()
            && let Some(existing) = values.get_mut(&key)
        {
            existing.attributes = parsed.attributes;
            existing.conditional = parsed.conditional;
        } else {
            values.insert(key, parsed);
        }
    }
    values
}

/// How an ability interacts with debuff immunity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebuffImmunity {
    /// Pierces immunity for the targets covered by the script flag.
    Pierces,
    /// Does not pierce immunity.
    Blocked,
    /// Pierces allied immunity only.
    AlliesOnly,
    /// A script value outside the known set.
    Other(String),
}

/// Which dispels can remove an ability's effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispel {
    /// A basic or strong dispel can remove the effect.
    Basic,
    /// Only a strong dispel can remove the effect.
    Strong,
    /// Dispels cannot remove the effect.
    None,
    /// A script value outside the known set.
    Other(String),
}

/// Damage type shown in the tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageType {
    /// Physical damage.
    Physical,
    /// Magical damage.
    Magical,
    /// Pure damage.
    Pure,
    /// Composite damage.
    Composite,
    /// Health removal.
    HealthRemoval,
    /// The script explicitly specifies no damage type.
    None,
    /// A script value outside the known set.
    Other(String),
}

impl DamageType {
    fn parse(value: &str) -> Self {
        match value {
            "DAMAGE_TYPE_PHYSICAL" => Self::Physical,
            "DAMAGE_TYPE_MAGICAL" => Self::Magical,
            "DAMAGE_TYPE_PURE" => Self::Pure,
            "DAMAGE_TYPE_COMPOSITE" => Self::Composite,
            "DAMAGE_TYPE_HP_REMOVAL" => Self::HealthRemoval,
            "DAMAGE_TYPE_NONE" => Self::None,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Script properties shown alongside a tooltip's description.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AbilityTags {
    /// Behavior flags including target mode, passive, toggle and channeling.
    pub behavior: Vec<String>,
    /// Target team flags.
    pub target_team: Vec<String>,
    /// Target unit type flags.
    pub target_type: Vec<String>,
    /// Additional unit target restrictions.
    pub target_flags: Vec<String>,
    /// Script damage type.
    pub damage_type: Option<DamageType>,
    /// Debuff immunity interaction, when the script specifies it.
    pub debuff_immunity: Option<DebuffImmunity>,
    /// Dispel interaction, when the script specifies it.
    pub dispel: Option<Dispel>,
    /// Whether the script marks the ability as innate or shows it in the innate UI.
    pub innate: bool,
    /// Whether Break disables the ability.
    pub breakable: Option<bool>,
    /// Whether Scepter upgrades this ability.
    pub has_scepter_upgrade: bool,
    /// Whether Shard upgrades this ability.
    pub has_shard_upgrade: bool,
    /// Whether Scepter grants this ability.
    pub granted_by_scepter: bool,
    /// Whether Shard grants this ability.
    pub granted_by_shard: bool,
    /// ID selecting the ability's numbered Scepter bonus.
    pub scepter_variant: Option<u32>,
    /// ID selecting the ability's numbered Shard bonus.
    pub shard_variant: Option<u32>,
    /// Script limit on ability level, if supplied.
    pub max_level: Option<usize>,
}

impl AbilityTags {
    fn from_fields(fields: &BTreeMap<String, String>) -> Self {
        let get = |key| fields.get(key).map(String::as_str);
        let flag = |key| get(key).is_some_and(truthy);
        let behavior = flags(get("AbilityBehavior"));
        Self {
            innate: flag("Innate")
                || behavior
                    .iter()
                    .any(|flag| flag == "DOTA_ABILITY_BEHAVIOR_INNATE_UI"),
            behavior,
            target_team: flags(get("AbilityUnitTargetTeam")),
            target_type: flags(get("AbilityUnitTargetType")),
            target_flags: flags(
                get("AbilityUnitTargetFlags").or_else(|| get("AbilityUnitTargetFlag")),
            ),
            damage_type: get("DamageTypeTooltip")
                .or_else(|| get("AbilityUnitDamageType"))
                .map(DamageType::parse),
            debuff_immunity: get("SpellImmunityType").map(|value| match value {
                "SPELL_IMMUNITY_ENEMIES_YES" | "SPELL_IMMUNITY_ALLIES_YES" => {
                    DebuffImmunity::Pierces
                }
                "SPELL_IMMUNITY_ENEMIES_NO" | "SPELL_IMMUNITY_ALLIES_NO" => DebuffImmunity::Blocked,
                "SPELL_IMMUNITY_ALLIES_YES_ENEMIES_NO" => DebuffImmunity::AlliesOnly,
                other => DebuffImmunity::Other(other.to_owned()),
            }),
            dispel: get("SpellDispellableType").map(|value| match value {
                "SPELL_DISPELLABLE_YES" => Dispel::Basic,
                "SPELL_DISPELLABLE_YES_STRONG" => Dispel::Strong,
                "SPELL_DISPELLABLE_NO" => Dispel::None,
                other => Dispel::Other(other.to_owned()),
            }),
            breakable: get("IsBreakable").map(truthy),
            has_scepter_upgrade: flag("HasScepterUpgrade"),
            has_shard_upgrade: flag("HasShardUpgrade"),
            granted_by_scepter: flag("IsGrantedByScepter"),
            granted_by_shard: flag("IsGrantedByShard"),
            scepter_variant: get("ScepterUpgradeID").and_then(|value| value.parse().ok()),
            shard_variant: get("ShardUpgradeID").and_then(|value| value.parse().ok()),
            max_level: get("MaxLevel").and_then(|value| value.parse().ok()),
        }
    }
}

/// A numbered Alt-note and its parsed text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Localization suffix number, including gaps in the sequence.
    pub number: u32,
    /// Tooltip text shown when holding Alt.
    pub text: RichText,
}

/// Localized fields shared by abilities, items, facets and modifiers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TooltipText {
    /// Display title, including the neutral noun form if needed.
    pub title: Option<RichText>,
    /// Main tooltip description.
    pub description: Option<RichText>,
    /// Lore or flavor text.
    pub lore: Option<RichText>,
    /// All numbered Alt-notes in numeric order.
    pub notes: Vec<Note>,
}

/// A contextual ability or item tooltip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AbilityTooltip {
    /// Main localized tooltip fields.
    pub text: TooltipText,
    /// Scepter description or upgrade text.
    pub scepter: Option<RichText>,
    /// Shard description or upgrade text.
    pub shard: Option<RichText>,
    /// Ability-specific facet descriptions, indexed by facet name.
    pub facets: BTreeMap<String, RichText>,
    /// Localized stat labels, keyed by lower-case value name.
    pub labels: BTreeMap<String, RichText>,
    /// Script values and stats, keyed by lower-case value name.
    pub values: BTreeMap<String, ScalingValue>,
    /// Tooltip behavior, targeting, damage and upgrade flags.
    pub tags: AbilityTags,
    linked_bonuses: BonusValues,
}

impl AbilityTooltip {
    /// Looks up a script stat or value, case-insensitively.
    pub fn value(&self, name: &str) -> Option<&ScalingValue> {
        self.values.get(&name.to_ascii_lowercase())
    }

    /// Renders a field with these script values and the live values supplied by the HUD.
    pub fn render(&self, text: &RichText, context: &Context<'_>) -> RenderedText {
        text.render(|variable| {
            context
                .runtime(variable)
                .or_else(|| self.resolve(variable, context))
        })
    }

    fn resolve(&self, variable: &Variable, context: &Context<'_>) -> Option<String> {
        let mut context = *context;
        context.scepter_variant = context.scepter_variant.or(self.tags.scepter_variant);
        context.shard_variant = context.shard_variant.or(self.tags.shard_variant);
        let name = variable.name.as_str();
        let exact = || {
            let values = self.value(name)?.resolve(&context);
            format_levels(&values, variable.format)
        };
        let bonus = || self.resolve_bonus(variable, &context);
        if variable.format == VariableFormat::Dialog {
            bonus().or_else(exact)
        } else {
            exact().or_else(bonus)
        }
    }

    fn resolve_bonus(&self, variable: &Variable, context: &Context<'_>) -> Option<String> {
        let name = variable.name.strip_prefix("bonus_")?;
        let linked = || {
            let variants = self.linked_bonuses.get(&name.to_ascii_lowercase())?;
            agreeing(variants.iter().filter_map(|variant| {
                let values = variant
                    .iter()
                    .map(|adjustment| adjustment.amount.abs())
                    .collect::<Vec<_>>();
                format_levels(&values, variable.format)
            }))
        };
        linked().or_else(|| {
            let values = self.value(name)?.resolve_with(context, true);
            format_levels(&values, variable.format)
        })
    }
}

fn agreeing(mut values: impl Iterator<Item = String>) -> Option<String> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn format_levels(values: &[f64], format: VariableFormat) -> Option<String> {
    if values.is_empty() || !values.iter().all(|value| value.is_finite()) {
        return None;
    }
    let mut output = Vec::new();
    for &value in values {
        let printed = if format == VariableFormat::Integer {
            format!("{:.0}", value.trunc())
        } else {
            let value = format!("{:.8}", if value == 0.0 { 0.0 } else { value });
            value.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        output.push(printed);
    }
    Some(output.join(" / "))
}

/// An ability associated with a facet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetAbility {
    /// Ability granted or modified by the facet.
    pub name: String,
    /// Ability this replaces, if specified.
    pub replaces: Option<String>,
    /// Whether the game automatically levels this ability.
    pub auto_level: Option<bool>,
}

/// One facet record in a hero script.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facet {
    /// Script facet name.
    pub name: String,
    /// Whether the build marks this facet deprecated.
    pub deprecated: bool,
    /// Icon name.
    pub icon: Option<String>,
    /// Color key.
    pub color: Option<String>,
    /// Gradient index.
    pub gradient: Option<u32>,
    /// Abilities granted or replaced when the facet is active.
    pub abilities: Vec<FacetAbility>,
}

/// A hero's declared ability slots, definitions and facets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hero {
    /// Whether the script enables this hero.
    pub enabled: bool,
    /// Numbered `AbilityN` slots by slot number, with empty slots omitted.
    pub slots: BTreeMap<u32, String>,
    /// Every ability defined in this hero script, in file order.
    pub definitions: Vec<String>,
    /// Facet records in script order, including deprecated facets.
    pub facets: Vec<Facet>,
}

/// Localized tooltips and their script inputs, independent of how the VPK was opened.
#[derive(Debug, Default)]
pub struct TooltipCatalog {
    localization: BTreeMap<String, String>,
    definitions: BTreeMap<String, Definition>,
    bonus_links: BTreeMap<String, BonusValues>,
    heroes: BTreeMap<String, Hero>,
}

type BonusValues = BTreeMap<String, Vec<Vec<Adjustment>>>;

#[derive(Debug, Default)]
struct Definition {
    base: Option<String>,
    fields: BTreeMap<String, String>,
    values: BTreeMap<String, ScalingValue>,
}

impl TooltipCatalog {
    /// Reads `abilities_english.txt`, `dota_english.txt`, and the item, global ability and
    /// individual hero scripts. Ability localization and the first definition win on duplicates.
    /// Hero files can be loaded directly from the VPK's `scripts/npc/heroes` directory.
    pub fn read(abilities: &[u8], dota: &[u8], scripts: &[&[u8]]) -> Result<Self> {
        let mut localization: BTreeMap<_, _> =
            localization_table(text(dota)?).into_iter().collect();
        localization.extend(localization_table(text(abilities)?));
        let mut output = Self {
            localization,
            ..Self::default()
        };
        let mut budget = Budget::named("tooltip scripts");
        for script in scripts {
            budget.charge(script.len())?;
            let tree = parse_kv_in(text(script)?, &mut budget)?;
            output.read_script(&tree);
        }
        output.index_bonuses();
        Ok(output)
    }

    /// Heroes by script name, including their slots and facet metadata.
    pub fn heroes(&self) -> &BTreeMap<String, Hero> {
        &self.heroes
    }

    /// All loaded item and ability definition names.
    pub fn definition_names(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }

    /// Parses a localization key, including shop and tooltip tag labels in `dota_english.txt`.
    pub fn localized(&self, key: &str) -> Option<RichText> {
        self.rich(&key.trim_start_matches('#').to_ascii_lowercase())
    }

    /// Recovers a bound enchantment's English title from its gender-form localization key.
    pub fn bound_enchantment(&self, name: &str) -> Option<RichText> {
        let key = format!("{PREFIX}{}_bound:f", name.to_ascii_lowercase());
        let value = self.localization.get(&key)?;
        let english = value
            .strip_prefix("{noun, gender, other{")?
            .strip_suffix("}}")?;
        Some(RichText::parse(english))
    }

    /// Looks up ability or item data including inherited script values and localized fields.
    pub fn ability(&self, name: &str) -> Option<AbilityTooltip> {
        let name = name.to_ascii_lowercase();
        let prefix = format!("{PREFIX}{name}");
        let chain = self.definition_chain(&name);
        let mut result = self.ability_text(&prefix);
        if chain.is_empty()
            && result.text == TooltipText::default()
            && result.scepter.is_none()
            && result.shard.is_none()
            && result.facets.is_empty()
        {
            return None;
        }
        for (base, _) in chain.iter().skip(1) {
            self.fill_base_text(&mut result, base);
        }
        let mut fields = BTreeMap::new();
        for (_, definition) in chain.iter().rev() {
            fields.extend(
                definition
                    .fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            for (name, value) in &definition.values {
                inherit_value(result.values.entry(name.clone()).or_default(), value);
            }
        }
        result.linked_bonuses = self.bonus_links.get(&name).cloned().unwrap_or_default();
        result.tags = AbilityTags::from_fields(&fields);
        result.labels = self.labels(&name, &prefix, result.values.keys());
        Some(result)
    }

    /// Looks up a facet's title, description, lore and notes by its script name.
    pub fn facet(&self, name: &str) -> Option<TooltipText> {
        let prefix = format!("dota_tooltip_facet_{}", name.to_ascii_lowercase());
        self.localized_text(&prefix)
    }

    /// Looks up a modifier/buff tooltip such as `modifier_axe_battle_hunger`.
    pub fn modifier(&self, name: &str) -> Option<TooltipText> {
        let name = name.to_ascii_lowercase();
        let name = name.strip_prefix("modifier_").unwrap_or(&name);
        self.localized_text(&format!("dota_tooltip_modifier_{name}"))
    }

    /// Resolves facet text against the hero's facet-granted, declared and defined abilities.
    /// A value shared by several candidate abilities must agree or be supplied by the caller.
    pub fn render_facet(
        &self,
        hero: &str,
        facet: &str,
        text: &RichText,
        context: &Context<'_>,
    ) -> RenderedText {
        if !text
            .spans
            .iter()
            .any(|span| matches!(span.content, TemplateContent::Variable(_)))
        {
            return text.render_context(context);
        }
        let context = Context {
            facet: Some(facet),
            ..*context
        };
        let mut abilities = Vec::new();
        if let Some(hero) = self.heroes.get(hero) {
            let mut seen = HashSet::new();
            let granted = hero
                .facets
                .iter()
                .find(|entry| entry.name == facet)
                .into_iter()
                .flat_map(|entry| entry.abilities.iter().map(|ability| &ability.name));
            for name in granted.chain(hero.slots.values()).chain(&hero.definitions) {
                if seen.insert(name)
                    && let Some(ability) = self.ability(name)
                {
                    abilities.push(ability);
                }
            }
        }
        text.render(|variable| {
            context.runtime(variable).or_else(|| {
                agreeing(
                    abilities
                        .iter()
                        .filter_map(|ability| ability.resolve_bonus(variable, &context)),
                )
                .or_else(|| {
                    agreeing(
                        abilities
                            .iter()
                            .filter_map(|ability| ability.resolve(variable, &context)),
                    )
                })
            })
        })
    }

    fn index_bonuses(&mut self) {
        for definition in self.definitions.values() {
            for (name, value) in &definition.values {
                for (key, expression) in &value.attributes {
                    if key.starts_with("special_bonus_") {
                        self.bonus_links
                            .entry(key.clone())
                            .or_default()
                            .entry(name.clone())
                            .or_default()
                            .push(adjustments(expression));
                    }
                }
            }
        }
    }

    fn read_script(&mut self, tree: &Kv) {
        self.read_definitions(tree.get("DOTAAbilities"));
        for (name, node) in children(tree.get("DOTAHeroes")) {
            if !name.starts_with("npc_dota_hero_") || node.block().is_none() {
                continue;
            }
            let definitions = node.get("AbilityDefinitions");
            self.read_definitions(definitions);
            self.heroes
                .entry(name.clone())
                .or_insert_with(|| read_hero(node, definitions));
        }
    }

    fn read_definitions(&mut self, tree: Option<&Kv>) {
        for (name, node) in children(tree) {
            if let Some(block) = node.block() {
                self.definitions
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| Definition {
                        base: field(node, "_base")
                            .or_else(|| field(node, "BaseClass"))
                            .map(str::to_ascii_lowercase),
                        fields: block
                            .iter()
                            .filter_map(|(key, value)| {
                                Some((key.clone(), value.as_str()?.trim().to_owned()))
                            })
                            .collect(),
                        values: read_scaling(node),
                    });
            }
        }
    }

    fn definition_chain<'a>(&'a self, name: &'a str) -> Vec<(&'a str, &'a Definition)> {
        let mut chain = Vec::new();
        let mut current = name;
        for _ in 0..16 {
            let Some(definition) = self.definitions.get(current) else {
                break;
            };
            if chain.iter().any(|(name, _)| *name == current) {
                break;
            }
            chain.push((current, definition));
            let Some(base) = &definition.base else { break };
            current = base;
        }
        chain
    }

    fn ability_text(&self, prefix: &str) -> AbilityTooltip {
        AbilityTooltip {
            text: self.text(prefix),
            scepter: self
                .rich(&format!("{prefix}_scepter_description"))
                .or_else(|| self.rich(&format!("{prefix}_aghanim_description"))),
            shard: self.rich(&format!("{prefix}_shard_description")),
            facets: self
                .suffixes(&format!("{prefix}_facet_"))
                .map(|(name, text)| (name.to_owned(), RichText::parse(text)))
                .collect(),
            ..AbilityTooltip::default()
        }
    }

    fn fill_base_text(&self, ability: &mut AbilityTooltip, base: &str) {
        let parent = self.ability_text(&format!("{PREFIX}{base}"));
        ability.text.title = ability.text.title.take().or(parent.text.title);
        ability.text.description = ability.text.description.take().or(parent.text.description);
        ability.text.lore = ability.text.lore.take().or(parent.text.lore);
        if ability.text.notes.is_empty() {
            ability.text.notes = parent.text.notes;
        }
        ability.scepter = ability.scepter.take().or(parent.scepter);
        ability.shard = ability.shard.take().or(parent.shard);
        for (name, text) in parent.facets {
            ability.facets.entry(name).or_insert(text);
        }
    }

    fn labels<'a>(
        &self,
        name: &str,
        prefix: &str,
        keys: impl Iterator<Item = &'a String>,
    ) -> BTreeMap<String, RichText> {
        let chain = self.definition_chain(name);
        keys.filter_map(|key| {
            let lookup = |prefix: &str| self.localization.get(&format!("{prefix}_{key}"));
            let label = lookup(prefix).or_else(|| {
                chain
                    .iter()
                    .find_map(|(name, _)| lookup(&format!("{PREFIX}{name}")))
            })?;
            Some((
                key.clone(),
                RichText::parse(&substitute(label, |name| {
                    self.localization
                        .get(&format!("{VARIABLE}{}", name.to_ascii_lowercase()))
                        .map(String::as_str)
                })),
            ))
        })
        .collect()
    }

    fn localized_text(&self, prefix: &str) -> Option<TooltipText> {
        let text = self.text(prefix);
        (text != TooltipText::default()).then_some(text)
    }

    fn text(&self, prefix: &str) -> TooltipText {
        let mut notes: Vec<_> = self
            .suffixes(&format!("{prefix}_note"))
            .filter_map(|(index, text)| {
                Some(Note {
                    number: index.parse().ok()?,
                    text: RichText::parse(text),
                })
            })
            .collect();
        notes.sort_by_key(|note| note.number);
        TooltipText {
            title: self
                .rich(prefix)
                .or_else(|| self.rich(&format!("{prefix}{NAME_FORM}"))),
            description: self.rich(&format!("{prefix}_description")),
            lore: self.rich(&format!("{prefix}_lore")),
            notes,
        }
    }

    fn rich(&self, key: &str) -> Option<RichText> {
        self.localization
            .get(key)
            .filter(|text| !text.trim().is_empty())
            .map(|text| RichText::parse(text))
    }

    fn suffixes<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.localization
            .range(prefix.to_owned()..)
            .map_while(move |(key, value)| Some((key.strip_prefix(prefix)?, value.as_str())))
    }
}

fn inherit_value(inherited: &mut ScalingValue, current: &ScalingValue) {
    if !current.levels.is_empty() {
        inherited.levels.clone_from(&current.levels);
    }
    for (key, value) in &current.conditional {
        inherit_value(inherited.conditional.entry(key.clone()).or_default(), value);
    }
    let mut attributes = HashMap::new();
    for (index, (name, _)) in inherited.attributes.iter().enumerate() {
        attributes.entry(name.clone()).or_insert(index);
    }
    for (name, value) in &current.attributes {
        let index = *attributes.entry(name.clone()).or_insert_with(|| {
            inherited.attributes.push((name.clone(), String::new()));
            inherited.attributes.len() - 1
        });
        inherited.attributes[index].1.clone_from(value);
    }
}

fn read_hero(node: &Kv, definitions: Option<&Kv>) -> Hero {
    let slots = children(Some(node))
        .iter()
        .filter_map(|(key, value)| {
            let number = key.strip_prefix("Ability")?.parse().ok()?;
            let value = value.as_str()?.trim();
            (!value.is_empty()).then(|| (number, value.to_owned()))
        })
        .collect();
    let definitions = children(definitions)
        .iter()
        .filter(|(_, node)| node.block().is_some())
        .map(|(name, _)| name.clone())
        .collect();
    let facets = children(node.get("Facets"))
        .iter()
        .filter(|(_, node)| node.block().is_some())
        .map(|(name, node)| read_facet(name, node))
        .collect();
    Hero {
        enabled: field(node, "Enabled").is_some_and(truthy),
        slots,
        definitions,
        facets,
    }
}

fn read_facet(name: &str, node: &Kv) -> Facet {
    let get = |key| field(node, key);
    Facet {
        name: name.to_owned(),
        deprecated: get("Deprecated").is_some_and(truthy),
        icon: get("Icon").map(str::to_owned),
        color: get("Color").map(str::to_owned),
        gradient: get("GradientID").and_then(|value| value.parse().ok()),
        abilities: children(node.get("Abilities"))
            .iter()
            .filter_map(|(_, ability)| {
                Some(FacetAbility {
                    name: field(ability, "AbilityName")?.to_owned(),
                    replaces: field(ability, "ReplaceAbility").map(str::to_owned),
                    auto_level: field(ability, "AutoLevelAbility").map(truthy),
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &RenderedText) -> String {
        text.spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn rich_text_preserves_nested_styles_and_expands_percent_escapes() {
        let text = RichText::parse(
            "<h1>Use: Blink</h1><br/><font color='#87ceeb'><b>%range%</b></font> / <i>%bonus_slow%%</i> / %other%%%",
        );
        let rendered = text.render_context(&Context {
            runtime: &[("range", "1200"), ("bonus_slow", "15"), ("other", "20")],
            ..Context::default()
        });
        assert_eq!(plain(&rendered), "Use: Blink\n1200 / 15% / 20%");
        assert_eq!(rendered.substitutions, 3);
        assert_eq!(rendered.spans[0].style.heading, Some(1));
        assert_eq!(rendered.spans[2].style.color, Some([135, 206, 235]));
        assert!(rendered.spans[2].style.bold);
        assert!(rendered.spans[4].style.italic);
        assert_eq!(rendered.spans[3].style, TextStyle::default());
    }

    #[test]
    fn modifier_properties_and_dialog_variables_accept_live_display_values() {
        let text = RichText::parse(
            "<span class=\"GameplayVariable\">%dMODIFIER_PROPERTY_TOOLTIP%%%, %fMODIFIER_PROPERTY_TOOLTIP2%, %s1s</span>",
        );
        let context = Context {
            runtime: &[("MODIFIER_PROPERTY_TOOLTIP", "14"), ("s1", "3")],
            ..Context::default()
        };
        let rendered = text.render_context(&context);
        assert_eq!(plain(&rendered), "14%, —, 3s");
        assert_eq!(
            rendered.unresolved,
            [Variable {
                name: "MODIFIER_PROPERTY_TOOLTIP2".to_owned(),
                format: VariableFormat::Decimal,
            }]
        );
        assert_eq!(
            rendered.spans[0].style.classes.as_deref(),
            Some("GameplayVariable")
        );
    }

    #[test]
    fn catalog_reads_innate_notes_upgrades_and_talent_effects_from_hero_scripts() {
        let localization = br#"
        "DOTA_Tooltip_ability_spell" "Innate"
        "DOTA_Tooltip_ability_spell_description" "Deals %damage% damage."
        "DOTA_Tooltip_ability_spell_note10" "Later"
        "DOTA_Tooltip_ability_spell_note2" "Earlier"
        "DOTA_Tooltip_ability_spell_scepter_description" "Deals %damage% damage."
        "DOTA_Tooltip_ability_spell_shard_description" "Cooldown: %AbilityCooldown%s."
        "DOTA_Tooltip_ability_special_bonus_example" "-{s:bonus_AbilityCooldown}s cooldown"
        "#;
        let hero = br#""DOTAHeroes" { "npc_dota_hero_example" {
            "Enabled" "1" "Ability7" "spell"
            "Facets" { "example_facet" { "Deprecated" "True" "GradientID" "1" } }
            "AbilityDefinitions" {
                "spell" {
                    "Innate" "1"
                    "AbilityCooldown" "10 8"
                    "AbilityUnitDamageType" "DAMAGE_TYPE_PURE"
                    "SpellDispellableType" "SPELL_DISPELLABLE_YES_STRONG"
                    "SpellImmunityType" "SPELL_IMMUNITY_ENEMIES_YES"
                    "AbilityValues" {
                        "damage" { "value" "50 80" "special_bonus_scepter_2" "+50%" }
                        "AbilityCooldown" { "special_bonus_example" "-2" "special_bonus_shard" "-1" }
                    }
                    "ScepterUpgradeID" "2"
                }
                "special_bonus_example" { "BaseClass" "special_bonus_base" }
            }
        } }"#;
        let catalog = TooltipCatalog::read(localization, b"", &[hero]).unwrap();
        let hero = &catalog.heroes()["npc_dota_hero_example"];
        assert!(hero.enabled && hero.facets[0].deprecated);
        assert_eq!(hero.slots[&7], "spell");
        let spell = catalog.ability("spell").unwrap();
        assert!(spell.tags.innate);
        assert_eq!(spell.tags.dispel, Some(Dispel::Strong));
        assert_eq!(spell.tags.debuff_immunity, Some(DebuffImmunity::Pierces));
        assert_eq!(spell.tags.damage_type, Some(DamageType::Pure));
        assert_eq!(
            spell
                .text
                .notes
                .iter()
                .map(|note| note.number)
                .collect::<Vec<_>>(),
            [2, 10]
        );
        let context = Context {
            level: Some(2),
            shard: true,
            scepter: true,
            ..Context::default()
        };
        assert_eq!(
            plain(&spell.render(spell.scepter.as_ref().unwrap(), &context)),
            "Deals 120 damage."
        );
        assert_eq!(
            plain(&spell.render(spell.shard.as_ref().unwrap(), &context)),
            "Cooldown: 7s."
        );
        let talent = catalog.ability("special_bonus_example").unwrap();
        assert_eq!(
            plain(&talent.render(talent.text.title.as_ref().unwrap(), &Context::default())),
            "-2s cooldown"
        );
    }

    #[test]
    fn inherited_stats_legacy_specials_and_facet_bonuses_share_one_context() {
        let localization = br#"
        "DOTA_Tooltip_ability_base" "Base"
        "DOTA_Tooltip_ability_parent" "Parent"
        "DOTA_Tooltip_facet_example" "Example"
        "DOTA_Tooltip_facet_example_description" "Gains {s:bonus_damage}% damage."
        "#;
        let hero = br#""DOTAHeroes" { "npc_dota_hero_example" {
            "Ability1" "leaf"
            "Facets" { "example" { "Abilities" { "Ability1" { "AbilityName" "leaf" } } } }
            "AbilityDefinitions" {
                "base" { "AbilitySpecial" { "01" { "var_type" "FIELD_INTEGER" "damage" "10 20" } } }
                "parent" { "_base" "base" }
                "leaf" { "_base" "parent" "AbilityValues" {
                    "damage" { "special_bonus_facet_example" { "value" "15" "special_bonus_friendly" "+5" } }
                    "growth" { "hero_levelup" "1" "levelup_interval" "3" }
                } }
            }
        } }"#;
        let catalog = TooltipCatalog::read(localization, b"", &[hero]).unwrap();
        let leaf = catalog.ability("leaf").unwrap();
        assert_eq!(
            plain(
                &leaf
                    .text
                    .title
                    .as_ref()
                    .unwrap()
                    .render_context(&Context::default())
            ),
            "Parent"
        );
        let context = Context {
            facet: Some("example"),
            hero_level: Some(7),
            bonuses: &["special_bonus_friendly"],
            ..Context::default()
        };
        assert_eq!(
            leaf.value("damage").unwrap().resolve(&context),
            [30.0, 40.0]
        );
        assert_eq!(leaf.value("growth").unwrap().resolve(&context), [2.0]);
        let facet = catalog.facet("example").unwrap();
        let rendered = catalog.render_facet(
            "npc_dota_hero_example",
            "example",
            &facet.description.unwrap(),
            &context,
        );
        assert_eq!(plain(&rendered), "Gains 20 / 20% damage.");
    }

    #[test]
    fn inherited_attributes_keep_order_and_first_duplicate_precedence() {
        let value = |rows: &[(&str, &str)]| ScalingValue {
            attributes: rows
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..ScalingValue::default()
        };
        let mut inherited = value(&[("a", "old"), ("a", "duplicate"), ("b", "keep")]);
        let current = value(&[("a", "new"), ("c", "first"), ("c", "last")]);
        inherit_value(&mut inherited, &current);
        assert_eq!(
            inherited.attributes,
            value(&[
                ("a", "new"),
                ("a", "duplicate"),
                ("b", "keep"),
                ("c", "last")
            ])
            .attributes
        );
        let before = inherited.clone();
        inherit_value(&mut inherited, &ScalingValue::default());
        assert_eq!(inherited, before);
    }
}
