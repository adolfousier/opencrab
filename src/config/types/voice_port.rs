//! Carry migration 2's provider enablement into the format-preserving
//! write (#1399).
//!
//! `migrate_if_needed` computes the `[voice]` to `providers.stt.*` /
//! `providers.tts.*` migration on a `toml::Value` document, then writes the
//! file through a separate `toml_edit` document so comments and ordering
//! survive. That second document only ever had the `[voice]` table removed
//! from it: the `enabled`, `model` and `voice` keys the migration set on the
//! first document never reached disk. A legacy config that said voice was
//! on came out of the migration with the table gone and nothing enabled,
//! and the deleted table left no second chance.

const KINDS: [&str; 2] = ["stt", "tts"];
const ENGINES: [&str; 5] = ["groq", "local", "openai_compatible", "voicebox", "openai"];

/// Copy every scalar under `providers.{stt,tts}.<engine>` from the migrated
/// value document into the edit document, creating the tables as needed.
/// Untouched keys already agree, so only the migration's writes change.
pub(crate) fn port_voice_providers(from: &toml::Value, into: &mut toml_edit::DocumentMut) {
    for kind in KINDS {
        let Some(engines) = from
            .get("providers")
            .and_then(|p| p.get(kind))
            .and_then(|k| k.as_table())
        else {
            continue;
        };
        for engine in ENGINES {
            let Some(values) = engines.get(engine).and_then(|e| e.as_table()) else {
                continue;
            };
            let table = ensure_table(into, &["providers", kind, engine]);
            for (key, value) in values {
                let item = match value {
                    toml::Value::Boolean(b) => toml_edit::value(*b),
                    toml::Value::String(s) => toml_edit::value(s.as_str()),
                    toml::Value::Integer(i) => toml_edit::value(*i),
                    toml::Value::Float(f) => toml_edit::value(*f),
                    _ => continue,
                };
                table.insert(key, item);
            }
        }
    }
}

fn ensure_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    path: &[&str],
) -> &'a mut toml_edit::Table {
    let mut current = doc.as_table_mut();
    for part in path {
        if !current.contains_key(part) {
            let mut fresh = toml_edit::Table::new();
            fresh.set_implicit(true);
            current.insert(part, toml_edit::Item::Table(fresh));
        }
        current = current
            .get_mut(part)
            .and_then(|i| i.as_table_mut())
            .expect("just inserted a table");
    }
    current
}
