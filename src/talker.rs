//! Response rules (`vrr_c`): which voice line a unit says when. A rule group names the
//! requirements a situation must meet (`Spawn`, `Isnevermore`) and the responses it picks
//! among (`SPEAK nevermore_nev_spawn_03`); the criteria say what each requirement matches.

use crate::budget::Budget;
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
    /// The requirements, by criterion name, all of which the situation must meet. Empty when
    /// the file states none: such a rule matches nothing (every criterion it meets scores).
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

/// The rules of a `vrr_c` resource, its KV3 `DATA` decoded within `budget`, and the rules' own
/// lists and texts charged to it before they are made: an entry of a few bytes in the document
/// may be a whole response in the rules.
pub fn read_response_rules(resource: &Resource, budget: &mut Budget) -> Result<ResponseRules> {
    let document = resource
        .kv3_in(b"DATA", budget)
        .ok_or_else(|| malformed("vrr", "no KV3 DATA block"))??;
    rules_of(&document.root, budget)
}

fn rules_of(data: &Value, budget: &mut Budget) -> Result<ResponseRules> {
    if data.as_object().is_none() {
        return Err(malformed("vrr", "not an object"));
    }
    let list = |key: &str, max: usize| -> Result<&[Value]> {
        let items = match data.get(key) {
            None => &[][..],
            Some(value) => value
                .as_array()
                .ok_or_else(|| malformed("vrr", format!("{key} is not an array")))?,
        };
        if items.len() > max {
            return Err(malformed("vrr", format!("{} {key}, too many", items.len())));
        }
        Ok(items)
    };
    let criteria = list("m_Requirements", MAX_CRITERIA)?;
    budget.slots::<Criterion>(criteria.len())?;
    let groups = list("m_ResponseGroups", MAX_GROUPS)?;
    budget.slots::<ResponseGroup>(groups.len())?;
    Ok(ResponseRules {
        includes: strings(list("m_Includes", MAX_LIST)?, budget)?,
        criteria: criteria
            .iter()
            .map(|c| criterion(c, budget))
            .collect::<Result<_>>()?,
        sound_event_scripts: strings(list("m_SoundEventScripts", MAX_LIST)?, budget)?,
        groups: groups
            .iter()
            .map(|g| group(g, budget))
            .collect::<Result<_>>()?,
    })
}

fn strings(items: &[Value], budget: &mut Budget) -> Result<Vec<String>> {
    budget.slots::<String>(items.len())?;
    items
        .iter()
        .filter_map(Value::as_str)
        .map(|text| budget.copy(text))
        .collect()
}

/// `v` itself when it is an object: an entry of `list` that is not one is not `entry` (a
/// criterion, a group, a response), and refused rather than read as an empty one.
fn object<'a>(v: &'a Value, list: &str, entry: &str) -> Result<&'a Value> {
    match v.as_object() {
        Some(_) => Ok(v),
        None => Err(malformed(
            "vrr",
            format!("{list} holds something that is not {entry}"),
        )),
    }
}

/// A group's list under `key`: none when the file leaves it out (KV3 leaves empty lists out),
/// an error when it is anything but a list, so that a rule never loses requirements unseen.
fn group_list<'a>(v: Option<&'a Value>, key: &str, max: usize) -> Result<&'a [Value]> {
    let items = match v.and_then(|v| v.get(key)) {
        None => &[][..],
        Some(value) => value
            .as_array()
            .ok_or_else(|| malformed("vrr", format!("group's {key} is not an array")))?,
    };
    if items.len() > max {
        return Err(malformed("vrr", "a group with more than any has"));
    }
    Ok(items)
}

/// Names that must each be one: a requirement that is not a name is an error, not dropped.
fn names(items: &[Value], key: &str, budget: &mut Budget) -> Result<Vec<String>> {
    budget.slots::<String>(items.len())?;
    items
        .iter()
        .map(|item| match item.as_str() {
            Some(name) => budget.copy(name),
            None => Err(malformed(
                "vrr",
                format!("group's {key} holds something that is not a name"),
            )),
        })
        .collect()
}

fn text(v: &Value, key: &str, budget: &mut Budget) -> Result<Option<String>> {
    v.get(key)
        .and_then(Value::as_str)
        .map(|text| budget.copy(text))
        .transpose()
}

fn number(v: Option<&Value>) -> Option<f32> {
    v?.as_f64().map(|n| finite(n as f32))
}

fn criterion(v: &Value, budget: &mut Budget) -> Result<Criterion> {
    let v = object(v, "m_Requirements", "a criterion")?;
    Ok(Criterion {
        name: text(v, "m_name", budget)?.unwrap_or_default(),
        match_key: text(v, "m_matchKey", budget)?,
        match_expr: text(v, "m_matchExpr", budget)?,
        weight: number(v.get("m_flWeight")),
        required: v.get("m_bRequired").and_then(Value::as_bool) == Some(true),
    })
}

fn group(v: &Value, budget: &mut Budget) -> Result<ResponseGroup> {
    let v = object(v, "m_ResponseGroups", "a response group")?;
    let rule = v.get("m_pEmbeddedRule");
    let requirements = group_list(rule, "m_Requirements", MAX_LIST)?;
    let responses = group_list(Some(v), "m_responses", MAX_RESPONSES)?;
    let name = text(v, "m_name", budget)?.unwrap_or_default();
    let rule = match rule {
        Some(rule) => text(rule, "m_name", budget)?.unwrap_or_default(),
        None => String::new(),
    };
    let requirements = names(requirements, "m_Requirements", budget)?;
    budget.slots::<Response>(responses.len())?;
    Ok(ResponseGroup {
        name,
        rule,
        requirements,
        responses: responses
            .iter()
            .map(|r| response(r, budget))
            .collect::<Result<_>>()?,
    })
}

fn response(v: &Value, budget: &mut Budget) -> Result<Response> {
    let v = object(v, "group's m_responses", "a response")?;
    let params = v.get("m_params");
    let start = |key: &str| {
        number(params.and_then(|p| p.get(&format!("{key}.start"))))
            .or_else(|| number(params.and_then(|p| p.get(key)?.get("start"))))
    };
    // The kinds Source names are no copy; another kind is kept by its name.
    let kind = match v.get("m_type").and_then(Value::as_str).unwrap_or_default() {
        "SPEAK" => ResponseKind::Speak,
        "SCENE" => ResponseKind::Scene,
        "PRINT" => ResponseKind::Print,
        "RESPONSE" => ResponseKind::Response,
        other => ResponseKind::Other(budget.copy(other)?),
    };
    let followup = match v.get("m_pConceptFollowup") {
        Some(f) => Some(Followup {
            target: text(f, "m_target", budget)?.unwrap_or_default(),
            concept: text(f, "m_concept", budget)?.unwrap_or_default(),
            contexts: text(f, "m_contexts", budget)?.unwrap_or_default(),
            delay: number(v.get("m_flFollowupDelay")).unwrap_or(0.0),
        }),
        None => None,
    };
    Ok(Response {
        kind,
        value: text(v, "m_value", budget)?,
        speak_once: params.and_then(|p| p.get("m_bSpeakOnce")?.as_bool()) == Some(true),
        respeak_delay: start("m_respeakdelay"),
        pre_delay: start("m_predelay"),
        weapon_delay: start("m_weapondelay"),
        followup,
    })
}
