//! The presentation readers of a resource: sound events, response rules and a particle's control
//! point configurations, whichever kind of file the input is. No input panics.
#![no_main]

use heatel::Resource;
use heatel::particle::control_point_configurations;
use heatel::sound::read_sound_events;
use heatel::talker::read_response_rules;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let _ = read_sound_events(&resource, &mut heatel::Budget::default());
    let _ = read_response_rules(&resource, &mut heatel::Budget::default());
    let _ = control_point_configurations(&resource);
});
