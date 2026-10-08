//! Work that changes the world (see `systems::world`): how each job looks while it is done and
//! how much of it there is.

use super::anims::AnimId;
use crate::state::WorldJob;

pub struct JobDef {
    pub job: WorldJob,
    /// present participle for status lines: "felling"
    pub verb: &'static str,
    pub anim: AnimId,
    /// "bundle/model" held while doing it
    pub tool: Option<&'static str>,
    /// villager-minutes the job takes, shared between everyone at it; 0 for one that is over at once
    pub work: f64,
    /// how many may work at it at once
    pub helpers: usize,
    /// how far from the thing one stands to work on it, beyond its own footprint
    pub reach: f64,
}

pub const JOBS: [JobDef; 8] = [
    JobDef {
        job: WorldJob::Fell,
        verb: "felling",
        anim: AnimId::Chop,
        tool: Some("tools/axe"),
        work: 90.0,
        helpers: 2,
        reach: 0.9,
    },
    JobDef {
        job: WorldJob::Mine,
        verb: "mining",
        anim: AnimId::Pickaxe,
        tool: Some("tools/pickaxe"),
        work: 120.0,
        helpers: 2,
        reach: 0.9,
    },
    JobDef {
        job: WorldJob::Dig,
        verb: "digging",
        anim: AnimId::Dig,
        tool: Some("tools/shovel"),
        work: 60.0,
        helpers: 1,
        reach: 1.2,
    },
    JobDef {
        job: WorldJob::Mow,
        verb: "cutting grass",
        anim: AnimId::Chop,
        tool: None,
        work: 45.0,
        helpers: 2,
        reach: 1.2,
    },
    JobDef {
        job: WorldJob::Dismantle,
        verb: "taking apart",
        anim: AnimId::Saw,
        tool: Some("tools/saw"),
        work: 40.0,
        helpers: 2,
        reach: 0.9,
    },
    JobDef {
        job: WorldJob::Burn,
        verb: "setting fire to",
        anim: AnimId::Interact,
        tool: Some("dungeon/torch_lit"),
        work: 0.0,
        helpers: 1,
        reach: 0.9,
    },
    JobDef {
        job: WorldJob::Build,
        verb: "building",
        anim: AnimId::Hammer,
        tool: Some("tools/hammer"),
        work: 0.0,
        helpers: 3,
        reach: 1.4,
    },
    JobDef {
        job: WorldJob::Deliver,
        verb: "carrying",
        anim: AnimId::Interact,
        tool: None,
        work: 0.0,
        helpers: 4,
        reach: 2.0,
    },
];

pub fn job(job: WorldJob) -> &'static JobDef {
    JOBS.iter().find(|def| def.job == job).expect("every job has a definition")
}
