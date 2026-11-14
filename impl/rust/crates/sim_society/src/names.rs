//! Names from a culture's naming rules (living-planet.md §3): syllables of onset, nucleus and
//! coda; places and factions from the culture's patterns.

use crate::lore::NamingRules;
use crate::rng::Rng;

/// A generated word: `syllables` (from the rules' range) of onset + nucleus + coda, the first
/// letter capitalised. Codas only close the last syllable and an occasional inner one, and
/// words that are hard to say (three vowels or four consonants in a row, a tripled letter,
/// more than ten letters) are drawn again.
pub fn word(rules: &NamingRules, rng: &mut Rng) -> String {
    let mut w = draw(rules, rng);
    for _ in 0..30 {
        if pronounceable(&w) {
            break;
        }
        w = draw(rules, rng);
    }
    capitalise(&w)
}

fn pronounceable(w: &str) -> bool {
    let vowel = |c: char| "aeiouy".contains(c);
    let (mut vowels, mut consonants, mut same, mut last) = (0, 0, 0, ' ');
    for c in w.chars() {
        if vowel(c) {
            vowels += 1;
            consonants = 0;
        } else {
            consonants += 1;
            vowels = 0;
        }
        same = if c == last { same + 1 } else { 1 };
        last = c;
        if vowels > 2 || consonants > 3 || same > 2 {
            return false;
        }
    }
    w.chars().count() <= 10
}

fn draw(rules: &NamingRules, rng: &mut Rng) -> String {
    let n = rng.range(
        rules.syllables[0],
        rules.syllables[1].max(rules.syllables[0]),
    );
    let mut w = String::new();
    for i in 0..n {
        w += rng.pick(&rules.onsets);
        w += rng.pick(&rules.nuclei);
        if i + 1 == n || rng.unit() < 0.25 {
            w += rng.pick(&rules.codas);
        }
    }
    w
}

/// A person's name: a given name and a family name.
pub fn person(rules: &NamingRules, rng: &mut Rng) -> String {
    format!("{} {}", word(rules, rng), word(rules, rng))
}

/// A place name from the rules' place patterns (the word and its ending at most 11 letters).
pub fn place(rules: &NamingRules, rng: &mut Rng) -> String {
    let pattern = rng.pick(&rules.place_patterns).clone();
    let ending = if pattern.contains("{ending}") {
        rng.pick(&rules.place_endings).clone()
    } else {
        String::new()
    };
    // Word and ending together stay short (Renby, not Lildsimundmere).
    let mut name = word(rules, rng);
    for _ in 0..30 {
        if name.chars().count() + ending.chars().count() <= 11 {
            break;
        }
        name = word(rules, rng);
    }
    pattern
        .replace("{name}", &name)
        .replace("{ending}", &ending)
}

/// A faction name from the rules' faction patterns, around the word `name`.
pub fn faction(rules: &NamingRules, name: &str, rng: &mut Rng) -> String {
    rng.pick(&rules.faction_patterns).replace("{name}", name)
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
