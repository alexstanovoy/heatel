//! Localized abilities, facets, modifiers and rich text from multipart scripts.
#![no_main]

use heatel::fuzzing::files;
use heatel::keyvalues::{localization_table, text};
use heatel::tooltips::{Context, RichText, TooltipCatalog};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parts: Vec<_> = files::split(data)
        .into_iter()
        .map(|(_, bytes)| bytes)
        .collect();
    let context = Context {
        level: Some(2),
        shard: true,
        scepter: true,
        ..Context::default()
    };
    if let Ok(template) = text(data) {
        let rich = RichText::parse(template);
        let _ = rich.render_context(&context);
        let _ = rich.render(|variable| Some(variable.name.clone()));
    }
    let dota = parts.get(1).copied().unwrap_or_default();
    let scripts = parts.get(2..).unwrap_or_default();
    let Ok(catalog) = TooltipCatalog::read(parts[0], dota, scripts) else {
        return;
    };
    let localized = text(parts[0]).map(localization_table).unwrap_or_default();
    let names = localized.keys().filter_map(|key| {
        key.strip_prefix("dota_tooltip_ability_")
            .or_else(|| key.strip_prefix("dota_tooltip_facet_"))
            .or_else(|| key.strip_prefix("dota_tooltip_modifier_"))
    });
    for name in catalog.definition_names().take(32).chain(names.take(32)) {
        if let Some(ability) = catalog.ability(name) {
            for value in ability.values.values().take(64) {
                let _ = value.resolve(&context);
            }
            if let Some(description) = &ability.text.description {
                let _ = ability.render(description, &context);
            }
        }
        let _ = catalog.facet(name);
        let _ = catalog.modifier(name);
    }
});
