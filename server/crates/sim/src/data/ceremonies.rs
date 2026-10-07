//! What is said at weddings and funerals when the model has written nothing. `{a}` and `{b}` are
//! the couple, `{name}` one person and `{names}` the dead, already joined with "and".

/// The officiant opens a wedding.
pub const WEDDING_OPENING: [&str; 3] = [
    "Friends, we are gathered on the green to see {a} and {b} wed. Stand close, and bear witness.",
    "Neighbours, come near. Today {a} and {b} bind their lives together before all of us.",
    "The valley has seen hard nights. Today it sees a wedding: {a} and {b}, before us all.",
];

/// One of the couple, to the other.
pub const VOWS: [&str; 5] = [
    "{name}, I will stand by you through every night the dead walk, and every morning after.",
    "{name}, whatever comes up the road, it finds us together. That is my promise.",
    "I have no fine words, {name}. I have my two hands and my whole heart, and they are yours.",
    "{name}, when the bell rings I will look for you first, always.",
    "I choose you, {name}, today and every day the valley gives us.",
];

/// The officiant pronounces them wed.
pub const WEDDING_CLOSING: [&str; 2] = [
    "Before the valley and all who watch over it, I name you wed. Now kiss, and let us eat!",
    "Then it is done: {a} and {b}, husband and wife, wife and husband, whatever you will. To the Tankard!",
];

/// The guests.
pub const CHEERS: [&str; 4] = [
    "Long life to them both!",
    "Hurrah for {a} and {b}!",
    "May your roof never burn and your well never run dry!",
    "To the tavern, before the ale is gone!",
];

/// The officiant opens a funeral.
pub const FUNERAL_OPENING: [&str; 2] = [
    "We lay {names} to rest in the earth of the valley. Stand with me a while.",
    "Neighbours, we have come to bury {names}. Let the dead hear that we remember them.",
];

/// The officiant speaks over the grave.
pub const EULOGIES: [&str; 3] = [
    "{names} lived among us and worked beside us. The valley is smaller this morning, and we are the poorer.",
    "Every one of us owed {names} a kindness we never repaid. Carry it forward to whoever comes next.",
    "The road brought {names} to us and the dark has taken them. We keep what was good, and we keep going.",
];

/// A mourner, to the dead.
pub const GRIEF: [&str; 5] = [
    "Sleep well, {name}. I will not forget you.",
    "I should have said more while you could hear it, {name}.",
    "Rest now, {name}. We will keep the watch.",
    "The valley will not be the same without you, {name}.",
    "Goodbye, {name}.",
];

/// The officiant closes a funeral.
pub const FUNERAL_CLOSING: [&str; 2] = [
    "Go in peace. The Tankard is open to all who would drink to their memory.",
    "That is all the words there are. Come, we will remember them better over a cup.",
];

/// A line with its names put in.
pub fn line(template: &str, names: &[(&str, &str)]) -> String {
    let mut text = template.to_owned();
    for (key, value) in names {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_renders_without_a_slot_left_over() {
        let names = [("a", "Elowen"), ("b", "Theron"), ("name", "Halvard"), ("names", "Halvard and Mira")];
        let all = WEDDING_OPENING
            .iter()
            .chain(&VOWS)
            .chain(&WEDDING_CLOSING)
            .chain(&CHEERS)
            .chain(&FUNERAL_OPENING)
            .chain(&EULOGIES)
            .chain(&GRIEF)
            .chain(&FUNERAL_CLOSING);
        for template in all {
            let text = line(template, &names);
            assert!(!text.contains('{') && !text.contains('}'), "{text}");
        }
    }
}
