#![allow(dead_code)]

pub fn mock_profile(instance: &str) -> sn::config::ResolvedProfile {
    sn::config::ResolvedProfile {
        name: "test".into(),
        instance: instance.to_string(),
        username: "admin".into(),
        password: "pw".into(),
    }
}
