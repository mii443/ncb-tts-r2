use crate::errors::constants::{MAX_SSML_LENGTH, MAX_TTS_TEXT_LENGTH};

const BREAK: &str = "<break time=\"200ms\"/>";

#[derive(Clone, Debug)]
enum Part {
    Text(String),
    Pause,
}

/// Literal text and trusted pauses are kept separate until engine-specific rendering.
#[derive(Clone, Debug, Default)]
pub struct SpeechText {
    parts: Vec<Part>,
    characters: usize,
}

pub fn bounded_text(text: &str) -> String {
    text.chars()
        .filter(|&c| valid_xml_char(c))
        .take(MAX_TTS_TEXT_LENGTH)
        .collect()
}

fn valid_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn escaped(c: char, buffer: &mut [u8; 4]) -> &str {
    match c {
        '&' => "&amp;",
        '<' => "&lt;",
        '>' => "&gt;",
        '"' => "&quot;",
        '\'' => "&apos;",
        _ => c.encode_utf8(buffer),
    }
}

pub fn escape_xml(text: &str, max_bytes: usize) -> String {
    let mut output = String::new();
    for c in text.chars().filter(|&c| valid_xml_char(c)) {
        let mut buffer = [0; 4];
        let entity = escaped(c, &mut buffer);
        if output.len() + entity.len() > max_bytes {
            break;
        }
        output.push_str(entity);
    }
    output
}

impl SpeechText {
    pub fn push_text(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|&c| valid_xml_char(c))
            .take(MAX_TTS_TEXT_LENGTH.saturating_sub(self.characters))
            .collect();
        self.characters += text.chars().count();
        self.parts.push(Part::Text(text));
    }

    pub fn pause(&mut self) {
        self.parts.push(Part::Pause);
    }

    pub fn plain(&self) -> String {
        let text: String = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.as_str(),
                Part::Pause => "、",
            })
            .collect();
        bounded_text(&text)
    }

    pub fn ssml(&self) -> String {
        let mut output = String::from("<speak>");
        for part in &self.parts {
            let remaining = MAX_SSML_LENGTH.saturating_sub(output.len() + "</speak>".len());
            match part {
                Part::Pause if BREAK.len() <= remaining => output.push_str(BREAK),
                Part::Pause => break,
                Part::Text(text) => {
                    let escaped = escape_xml(text, remaining);
                    let complete = escape_xml(text, usize::MAX).len() == escaped.len();
                    output.push_str(&escaped);
                    if !complete {
                        break;
                    }
                }
            }
        }
        output.push_str("</speak>");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_and_emoji_are_bounded_at_character_boundaries() {
        assert_eq!(bounded_text(&"あ".repeat(167)), "あ".repeat(167));
        let mut text = SpeechText::default();
        text.push_text(&"あ🙂".repeat(500));
        assert_eq!(text.plain().chars().count(), MAX_TTS_TEXT_LENGTH);
        let ssml = text.ssml();
        assert!(ssml.len() <= MAX_SSML_LENGTH);
        assert!(ssml.ends_with("</speak>"));
    }

    #[test]
    fn only_generated_markup_is_preserved() {
        let mut text = SpeechText::default();
        text.push_text("A & B < C\u{0}");
        text.pause();
        text.push_text("<audio src=\"x\"/>🙂");
        assert_eq!(text.ssml(), "<speak>A &amp; B &lt; C<break time=\"200ms\"/>&lt;audio src=&quot;x&quot;/&gt;🙂</speak>");
        assert_eq!(escape_xml("&&", 7), "&amp;");
        assert_eq!(escape_xml("あ", 2), "");
    }
}
