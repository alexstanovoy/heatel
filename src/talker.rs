//! Response rules (`vrr_c`): which voice line a unit says when. A rule group names the
//! requirements a situation must meet (`Spawn`, `Isnevermore`) and the responses it picks
//! among (`SPEAK nevermore_nev_spawn_03`); the criteria say what each requirement matches.

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::finite;
use crate::resource::Resource;

/// Most response groups one file holds (the largest of the install has a few thousand).
const MAX_GROUPS: usize = 1 << 16;
/// Most criteria one file defines.
const MAX_CRITERIA: usize = 1 << 14;
/// Most responses one group picks among.
const MAX_RESPONSES: usize = 1 << 12;
/// Most requirements, includes or scripts one list holds.
const MAX_LIST: usize = 1 << 12;

/// The rules of one `vrr_c` file.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseRules {
    /// The rule files whose groups and criteria this one builds on
    /// (`scripts/talker/response_rules_shared.vrr`).
    pub includes: Vec<String>,
    /// The criteria this file defines.
    pub criteria: Vec<Criterion>,
    /// The sound event scripts the voice files name their events in.
    pub sound_event_scripts: Vec<String>,
    /// The response groups, each with the rule that selects it, in file order.
    pub groups: Vec<ResponseGroup>,
}

/// What a requirement checks: a fact of the situation against an expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    /// The requirement's name, which a rule lists (`Isnevermore`).
    pub name: String,
    /// The fact it reads (`classname`, `abilityname`); a concept's own name when absent.
    pub match_key: Option<String>,
    /// What the fact must match (`npc_dota_hero_Nevermore`).
    pub match_expr: Option<String>,
    /// How much it weighs in choosing the best rule.
    pub weight: Option<f32>,
    /// Whether the rule fails without it, whatever it weighs.
    pub required: bool,
}

/// A rule and the responses it picks among.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseGroup {
    /// The group's name (`nevermore_Spawn`).
    pub name: String,
    /// The name of its rule.
    pub rule: String,
    /// The requirements, by criterion name, all of which the situation must meet.
    pub requirements: Vec<String>,
    /// The responses, one of which plays.
    pub responses: Vec<Response>,
}

/// One response of a group.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// What it does.
    pub kind: ResponseKind,
    /// The voice event, scene or text it names.
    pub value: Option<String>,
    /// Whether it plays at most once.
    pub speak_once: bool,
    /// Seconds before the same unit speaks again.
    pub respeak_delay: Option<f32>,
    /// Seconds before the line starts.
    pub pre_delay: Option<f32>,
    /// Seconds a weapon swing delays it.
    pub weapon_delay: Option<f32>,
    /// A concept another unit answers with.
    pub followup: Option<Followup>,
}

/// What a response does.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseKind {
    /// `SPEAK`: plays a voice event.
    Speak,
    /// `SCENE`: plays a choreographed scene.
    Scene,
    /// `PRINT`: prints text (a developer's).
    Print,
    /// `RESPONSE`: picks from another group.
    Response,
    /// Any other kind, by its name.
    Other(String),
}

/// A line another unit is asked to answer with.
#[derive(Debug, Clone, PartialEq)]
pub struct Followup {
    /// Who answers.
    pub target: String,
    /// The concept it is asked.
    pub concept: String,
    /// The contexts it is asked with.
    pub contexts: String,
    /// Seconds after the line.
    pub delay: f32,
}

/// The rules of a `vrr_c` resource.
pub fn read_response_rules(resource: &Resource) -> Result<ResponseRules> {
    let data = resource
        .data()
        .ok_or_else(|| malformed("vrr", "no DATA block"))??;
    if data.as_object().is_none() {
        return Err(malformed("vrr", "not an object"));
    }
    let list = |key: &str, max: usize| -> Result<&[Value]> {
        let items = data.get(key).and_then(Value::as_array).unwrap_or_default();
        if items.len() > max {
            return Err(malformed("vrr", format!("{} {key}, too many", items.len())));
        }
        Ok(items)
    };
    Ok(ResponseRules {
        includes: strings(list("m_Includes", MAX_LIST)?),
        criteria: list("m_Requirements", MAX_CRITERIA)?
            .iter()
            .map(criterion)
            .collect(),
        sound_event_scripts: strings(list("m_SoundEventScripts", MAX_LIST)?),
        groups: list("m_ResponseGroups", MAX_GROUPS)?
            .iter()
            .map(group)
            .collect::<Result<_>>()?,
    })
}

fn strings(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn number(v: Option<&Value>) -> Option<f32> {
    v?.as_f64().map(|n| finite(n as f32))
}

fn criterion(v: &Value) -> Criterion {
    Criterion {
        name: text(v, "m_name").unwrap_or_default(),
        match_key: text(v, "m_matchKey"),
        match_expr: text(v, "m_matchExpr"),
        weight: number(v.get("m_flWeight")),
        required: v.get("m_bRequired").and_then(Value::as_bool) == Some(true),
    }
}

fn group(v: &Value) -> Result<ResponseGroup> {
    let rule = v.get("m_pEmbeddedRule");
    let requirements = rule.and_then(|r| r.get("m_Requirements"));
    let requirements = requirements.and_then(Value::as_array).unwrap_or_default();
    let responses = v.get("m_responses").and_then(Value::as_array);
    let responses = responses.unwrap_or_default();
    if requirements.len() > MAX_LIST || responses.len() > MAX_RESPONSES {
        return Err(malformed("vrr", "a group with more than any has"));
    }
    Ok(ResponseGroup {
        name: text(v, "m_name").unwrap_or_default(),
        rule: rule.and_then(|r| text(r, "m_name")).unwrap_or_default(),
        requirements: strings(requirements),
        responses: responses.iter().map(response).collect(),
    })
}

fn response(v: &Value) -> Response {
    let params = v.get("m_params");
    let start = |key: &str| number(params.and_then(|p| p.get(key)?.get("start")));
    let kind = match text(v, "m_type").unwrap_or_default().as_str() {
        "SPEAK" => ResponseKind::Speak,
        "SCENE" => ResponseKind::Scene,
        "PRINT" => ResponseKind::Print,
        "RESPONSE" => ResponseKind::Response,
        other => ResponseKind::Other(other.to_owned()),
    };
    Response {
        kind,
        value: text(v, "m_value"),
        speak_once: params.and_then(|p| p.get("m_bSpeakOnce")?.as_bool()) == Some(true),
        respeak_delay: start("m_respeakdelay"),
        pre_delay: start("m_predelay"),
        weapon_delay: start("m_weapondelay"),
        followup: v.get("m_pConceptFollowup").map(|f| Followup {
            target: text(f, "m_target").unwrap_or_default(),
            concept: text(f, "m_concept").unwrap_or_default(),
            contexts: text(f, "m_contexts").unwrap_or_default(),
            delay: number(v.get("m_flFollowupDelay")).unwrap_or(0.0),
        }),
    }
}
