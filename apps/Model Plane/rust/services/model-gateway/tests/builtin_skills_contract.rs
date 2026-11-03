//! Pins the first-party Office skills to the files the execution-core image copies.
//!
//! The gateway embeds each SKILL.md with `include_str!`. The execution-core image
//! copies the whole `skills/office` directory to `/opt/verevon/skills/office`, where
//! the sandbox can read it. If the two drift, the model is told one rule and reads
//! another from inside the sandbox, so this test fails the build instead.

use std::path::{Path, PathBuf};

use model_gateway::skills::builtin_skills::OFFICE_SKILLS;

/// The Model Plane root: the build context of both images.
fn model_plane_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
}

#[test]
fn the_execution_core_image_copies_the_office_directory() {
    let dockerfile =
        std::fs::read_to_string(model_plane_root().join("rust/services/execution-core/Dockerfile"))
            .expect("execution-core Dockerfile");
    assert!(
        dockerfile
            .lines()
            .any(|line| line.trim_end() == "COPY skills/office/ /opt/verevon/skills/office/"),
        "the execution-core image must copy skills/office to /opt/verevon/skills/office, \
         the path the sandbox reads a skill's files from"
    );
}

#[test]
fn every_embedded_skill_is_the_file_the_image_copies() {
    for skill in OFFICE_SKILLS.iter() {
        let copied = read(
            &model_plane_root()
                .join("skills/office")
                .join(skill.dir)
                .join("SKILL.md"),
        );
        assert_eq!(
            skill.text.as_bytes(),
            copied.as_slice(),
            "the gateway embeds a SKILL.md that differs from skills/office/{}/SKILL.md, \
             the file the image ships",
            skill.dir
        );
    }
}

#[test]
fn every_office_skill_on_disk_is_embedded() {
    let mut on_disk: Vec<String> = std::fs::read_dir(model_plane_root().join("skills/office"))
        .expect("skills/office")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut embedded: Vec<String> = OFFICE_SKILLS
        .iter()
        .map(|skill| skill.dir.to_owned())
        .collect();
    embedded.sort();
    assert_eq!(
        on_disk, embedded,
        "a SKILL.md ships in the image without being registered, or a registered one is missing"
    );
}

#[test]
fn each_skill_is_named_as_its_front_matter_declares() {
    for skill in OFFICE_SKILLS.iter() {
        let declared = skill
            .text
            .lines()
            .find_map(|line| line.strip_prefix("name: "));
        assert_eq!(
            declared,
            Some(skill.name),
            "{}: the registered name must be the one its SKILL.md declares",
            skill.dir
        );
    }
}

/// The pptx skill runs its builder from the path the execution-core image copies the
/// skill to. The builder must sit beside that SKILL.md, and the text must name that path.
#[test]
fn the_pptx_skill_ships_and_names_its_builder() {
    let builder = model_plane_root().join("skills/office/pptx/scripts/build_deck.py");
    assert!(
        builder.is_file(),
        "the pptx skill's builder must sit beside its SKILL.md: {}",
        builder.display()
    );
    let pptx = OFFICE_SKILLS
        .iter()
        .find(|skill| skill.dir == "pptx")
        .expect("the pptx skill is registered");
    assert!(
        pptx.text
            .contains("/opt/verevon/skills/office/pptx/scripts/build_deck.py"),
        "the pptx SKILL.md must run the builder from /opt/verevon/skills/office/pptx/scripts/"
    );
}
