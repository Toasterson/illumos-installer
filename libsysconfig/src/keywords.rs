use libcfgparser::KeywordDefinition;

pub fn get_supported_keywords() -> Vec<(String, KeywordDefinition)> {
    vec![
        ("keyboard".into(), KeywordDefinition { options: vec![] }),
        ("timezone".into(), KeywordDefinition { options: vec![] }),
        ("terminal".into(), KeywordDefinition { options: vec![] }),
        ("timeserver".into(), KeywordDefinition { options: vec![] }),
        (
            "system_locale".into(),
            KeywordDefinition { options: vec![] },
        ),
        (
            "network_interface".into(),
            KeywordDefinition {
                options: vec![
                    "name".into(),
                    "static".into(),
                    "static6".into(),
                    "primary".into(),
                ],
            },
        ),
        (
            "dataset".into(),
            KeywordDefinition {
                options: vec![
                    "aclinherit".into(),
                    "aclmode".into(),
                    "atime".into(),
                    "canmount".into(),
                    "checksum".into(),
                    "compression".into(),
                    "copies".into(),
                    "devices".into(),
                    "encryption".into(),
                    "keyformat".into(),
                    "keylocation".into(),
                    "exec".into(),
                    "filesystem_limit".into(),
                    "special_small_blocks".into(),
                    "mountpoint".into(),
                    "nbmand".into(),
                    "pbkdf2iters".into(),
                    "primarycache".into(),
                    "quota".into(),
                    "snapshot_limit".into(),
                    "readonly".into(),
                    "recordsize".into(),
                    "redundant_metadata".into(),
                    "refquota".into(),
                    "refreservation".into(),
                    "reservation".into(),
                    "secondarycache".into(),
                    "setuid".into(),
                    "sharesmb".into(),
                    "sharenfs".into(),
                    "logbias".into(),
                    "snapdir".into(),
                    "sync".into(),
                    "vscan".into(),
                    "xattr".into(),
                    "casesensitivity".into(),
                    "normalization".into(),
                    "utf8only".into(),
                ],
            },
        ),
        (
            "setup_dns".into(),
            KeywordDefinition {
                options: vec!["search".into(), "domain".into()],
            },
        ),
        ("route".into(), KeywordDefinition { options: vec![] }),
        (
            "root_password".into(),
            KeywordDefinition { options: vec![] },
        ),
    ]
}
