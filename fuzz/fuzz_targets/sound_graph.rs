//! Event inheritance, operator stack imports and effective playback fields.
#![no_main]

use heatel::fuzzing::files;
use heatel::sound::{SoundEventGraph, SoundTrack};
use heatel::{Budget, Resource, kv3};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut graph = SoundEventGraph::default();
    let mut budget = Budget::new(32 << 20);
    let parts = files::split(data);
    for bytes in std::iter::once(data).chain(parts.iter().map(|(_, bytes)| *bytes)) {
        if let Ok(resource) = Resource::parse(bytes) {
            let _ = graph.read_events(&resource, &mut budget);
            let _ = graph.read_stacks(&resource, &mut budget);
        } else if let Ok(document) = kv3::decode_in(bytes, &mut budget) {
            let _ = graph.add_events(document.root.clone(), &mut budget);
            let _ = graph.add_stacks(document.root, &mut budget);
        }
    }
    for name in graph
        .event_names()
        .take(64)
        .chain(std::iter::once("missing"))
    {
        if let Ok(Some(event)) = graph.resolve(name) {
            let _ = event.base_chain().count();
            let _ = event.stack_definition();
            let _ = event.playback();
            let _ = event.limits();
            for track in [SoundTrack::Main, SoundTrack::Track01] {
                if let Some(track) = event.track(track) {
                    let _ = track.selection_mode();
                    let _ = track.files().count();
                }
            }
        }
    }
});
