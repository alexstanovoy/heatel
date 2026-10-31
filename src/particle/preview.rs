//! A particle system's control point configurations (`m_controlPointConfigurations`): the
//! control points the effect expects and how an editor attaches them to preview it. They say
//! which points a system reads (Shadowraze: 0, 3, 60, 61), not what the game feeds them.

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::finite;
use crate::resource::Resource;

use super::read::{MAX_CONTROL_POINT, WHAT};

/// Most configurations a system has.
const MAX_CONFIGS: usize = 64;
/// Most drivers (control points) one configuration sets.
const MAX_DRIVERS: usize = 256;

/// A named setup of control points.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlPointConfig {
    /// Its name (`preview`).
    pub name: String,
    /// The control points it sets, in file order.
    pub drivers: Vec<ControlPointDriver>,
    /// The model and sequence the editor shows it on, when it names any.
    pub preview_model: Option<String>,
    /// The sequence the editor plays on that model.
    pub preview_sequence: Option<String>,
}

/// How one control point is set.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlPointDriver {
    /// The control point; 0 when the file leaves it out.
    pub control_point: u8,
    /// Dota's attach type (`PATTACH_WORLDORIGIN`, `PATTACH_ABSORIGIN_FOLLOW`); none when the
    /// file leaves it out.
    pub attach_type: Option<String>,
    /// The entity it follows (`self`).
    pub entity: Option<String>,
    /// The model attachment it sits on (`attach_hitloc`).
    pub attachment: Option<String>,
    /// Its offset from there.
    pub offset: [f32; 3],
    /// Its angles.
    pub angles: [f32; 3],
}

/// The control point configurations of a `vpcf_c` resource; none when it has none.
pub fn control_point_configurations(resource: &Resource<'_>) -> Result<Vec<ControlPointConfig>> {
    let data = resource
        .kv3(b"DATA")
        .ok_or_else(|| malformed(WHAT, "no KV3 DATA block"))??;
    let Some(configs) = data
        .root
        .get("m_controlPointConfigurations")
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    if configs.len() > MAX_CONFIGS {
        return Err(malformed(
            WHAT,
            "more control point configurations than any has",
        ));
    }
    configs.iter().map(config).collect()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn config(v: &Value) -> Result<ControlPointConfig> {
    let drivers = v.get("m_drivers").and_then(Value::as_array);
    let drivers = drivers.unwrap_or_default();
    if drivers.len() > MAX_DRIVERS {
        return Err(malformed(WHAT, "more control point drivers than any has"));
    }
    let state = v.get("m_previewState");
    Ok(ControlPointConfig {
        name: text(v, "m_name").unwrap_or_default(),
        drivers: drivers.iter().map(driver).collect::<Result<_>>()?,
        preview_model: state
            .and_then(|s| text(s, "m_previewModel"))
            .or_else(|| text(v, "m_previewModel")),
        preview_sequence: state.and_then(|s| text(s, "m_sequenceName")),
    })
}

fn driver(v: &Value) -> Result<ControlPointDriver> {
    let point = v
        .get("m_iControlPoint")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let control_point = u8::try_from(point)
        .ok()
        .filter(|p| *p <= MAX_CONTROL_POINT)
        .ok_or_else(|| malformed(WHAT, format!("control point {point}")))?;
    Ok(ControlPointDriver {
        control_point,
        attach_type: text(v, "m_iAttachType"),
        entity: text(v, "m_entityName"),
        attachment: text(v, "m_attachmentName").filter(|a| !a.is_empty()),
        offset: vector(v.get("m_vecOffset")),
        angles: vector(v.get("m_angOffset")),
    })
}

/// Three numbers; zeros for anything else.
fn vector(v: Option<&Value>) -> [f32; 3] {
    let mut out = [0.0; 3];
    let items = v.and_then(Value::as_array).unwrap_or_default();
    if items.len() == 3 {
        for (o, i) in out.iter_mut().zip(items) {
            *o = finite(i.as_f64().unwrap_or(0.0) as f32);
        }
    }
    out
}
