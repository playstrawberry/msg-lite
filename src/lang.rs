//! The script's texts: `lang/{en,fr,es,tr}.lang`, embedded and checked for the same keys while it compiles.
use sp_lang::{Catalog, Language};

static LANG: Catalog = sp_lang::catalog!("msg-lite");

/// The text of `key` in the language of `locale` (English for any other), with `{0}`, `{1}` filled from
/// `args`. An argument is put in once and never read as a template again.
pub fn text(locale: &str, key: &str, args: &[&str]) -> String {
    let raw = LANG
        .get_or_en(Language::of_locale(locale), key)
        .unwrap_or_default();
    sp_lang::render(raw, args, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_the_script_names_has_a_text_in_every_language() {
        let own = [
            "description",
            "permission_use",
            "command_msg",
            "command_reply",
            "players_only",
            "no_permission",
            "to",
            "from",
            "spy",
            "not_found",
            "self",
            "empty",
            "too_long",
            "no_reply",
        ];
        for key in own {
            for language in Language::ALL {
                assert!(
                    LANG.get(language, key).is_some_and(|t| !t.is_empty()),
                    "{key} in {}",
                    language.code()
                );
            }
        }
        assert_eq!(LANG.len(), own.len(), "a key nobody names");
    }

    #[test]
    fn a_message_is_shown_as_typed_in_every_language() {
        // What a player typed is text: a placeholder or a colour code in it is not read.
        let typed = "{0} {1} &c%s";
        for locale in ["en_us", "fr_fr", "es_es", "tr_tr"] {
            assert!(
                text(locale, "to", &["Bob", typed]).ends_with(typed),
                "{locale}"
            );
            assert!(
                text(locale, "from", &["Ann", typed]).ends_with(typed),
                "{locale}"
            );
            let spy = text(locale, "spy", &["Ann", "Bob", typed]);
            assert!(spy.ends_with(typed) && spy.contains("Ann") && spy.contains("Bob"));
        }
        assert_eq!(text("en_us", "to", &["Bob", "hi"]), "[me -> Bob] hi");
        assert_eq!(text("en_us", "from", &["Ann", "hi"]), "[Ann -> me] hi");
        assert_eq!(
            text("en_us", "spy", &["Ann", "Bob", "hi"]),
            "[spy] Ann -> Bob: hi"
        );
    }

    #[test]
    fn the_script_is_the_one_social_spy_answers() {
        assert_eq!(sp_social::MESSAGE_PLUGIN, "msg-lite");
    }
}
