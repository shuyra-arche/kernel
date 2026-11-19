use kernel_verification::{Result, require};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Suite {
    pub id: String,
    pub profiles: Vec<String>,
    pub harts: Vec<u8>,
    pub sources: Vec<String>,
    pub run: Run,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Run {
    Qemu {
        kernel: String,
        guest: Option<Guest>,
        #[serde(default)]
        imports: Vec<String>,
        #[serde(default)]
        features: Vec<String>,
        #[serde(default)]
        environment: BTreeMap<String, String>,
        #[serde(default)]
        patch_provider: bool,
        expects: Vec<String>,
        distinct: Option<Distinct>,
    },
    CargoTest {
        package: String,
        test: String,
        ignored: bool,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct Guest {
    pub binary: String,
    pub capsule: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Distinct {
    pub pattern: String,
    pub count: usize,
}

pub fn read(path: &Path) -> Result<(Suite, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing test root")?
        .canonicalize()?;
    let target = root.join("target").canonicalize()?;
    let directory = path.canonicalize()?;
    require(
        directory.starts_with(&target),
        "test project must be inside server/tests/target",
    )?;
    let relative = directory.strip_prefix(&root)?.to_owned();
    let suite: Suite = serde_json::from_slice(&fs::read(directory.join("suite.json"))?)?;
    require(
        !suite.id.is_empty()
            && suite
                .id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        "invalid test project id",
    )?;
    require(
        !suite.profiles.is_empty()
            && suite
                .profiles
                .iter()
                .all(|profile| profile == "debug" || profile == "release"),
        "invalid test profiles",
    )?;
    require(
        !suite.harts.is_empty()
            && suite.harts.iter().all(|hart| matches!(hart, 1 | 4)),
        "invalid test Hart selection",
    )?;
    require(!suite.sources.is_empty(), "test project has no source owners")?;
    if let Run::Qemu {
        imports, expects, ..
    } = &suite.run
    {
        require(!expects.is_empty(), "QEMU suite requires result assertions")?;
        for import in imports {
            let path = Path::new(import);
            require(
                path.components().all(|part| matches!(part, Component::Normal(_))),
                "test imports must stay within the test tree",
            )?;
        }
    }
    Ok((suite, relative))
}
