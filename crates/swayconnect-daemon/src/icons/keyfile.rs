//! The group-based key file format shared by desktop entries and `index.theme`
//! (Desktop Entry Specification, "Basic format of the file").

/// The keys of one `[group]`, in file order. Localised keys (`Name[de]`) are dropped:
/// the icon service reads only unlocalised ones.
#[derive(Debug, Default)]
pub(crate) struct Group {
    pub(crate) name: String,
    entries: Vec<(String, String)>,
}

impl Group {
    /// The value of the first `key`, trimmed.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// Every group of `text`. Lines before the first group, comments and lines without `=`
/// are skipped, as a lenient reader of untrusted files does.
pub(crate) fn parse(text: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            groups.push(Group {
                name: name.to_owned(),
                entries: Vec::new(),
            });
            continue;
        }
        let (Some(group), Some((key, value))) = (groups.last_mut(), line.split_once('=')) else {
            continue;
        };
        let key = key.trim();
        if !key.contains('[') {
            group
                .entries
                .push((key.to_owned(), value.trim().to_owned()));
        }
    }
    groups
}

/// The first group called `name`.
pub(crate) fn group<'a>(groups: &'a [Group], name: &str) -> Option<&'a Group> {
    groups.iter().find(|group| group.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_keep_unlocalised_keys_and_skip_noise() {
        let text = "orphan=1\n# comment\n[Desktop Entry]\nIcon = notes \nName[de]=Notizen\n\
                    no equals sign\n[Other]\nIcon=other\n";
        let groups = parse(text);
        assert_eq!(groups.len(), 2);
        let entry = group(&groups, "Desktop Entry").expect("group");
        assert_eq!(entry.get("Icon"), Some("notes"));
        assert_eq!(entry.get("Name"), None);
        assert_eq!(entry.get("orphan"), None);
        assert_eq!(
            group(&groups, "Other").and_then(|g| g.get("Icon")),
            Some("other")
        );
    }
}
