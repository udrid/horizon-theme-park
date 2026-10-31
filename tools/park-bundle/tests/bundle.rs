// Tests for park-bundle. They glue a small made-up crate (the "sample tree" in tests/sample)
// and check what comes out. Numbers 1-8 match the list in docs/milestones/M0.md section 7.

use park_bundle::{BundleError, GAME_PREFIX, OUTPUT_FILE, SOURCE_DIR, bundle, find_refusal, run};
use std::fs;
use std::path::{Path, PathBuf};

/// The checked-in sample crate.
fn sample_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample")
}

/// Build a throw-away folder of files under `target/`, from (path, contents) pairs.
/// Each test uses its own `name`, so tests running side by side never share a folder.
fn make_tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("park-bundle-{name}"));
    let _ = fs::remove_dir_all(&dir);
    for (path, contents) in files {
        let full = dir.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    dir
}

/// Copy a folder of text files, turning every Linux line ending into a Windows one.
fn copy_with_windows_line_endings(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_with_windows_line_endings(&entry.path(), &target);
        } else {
            let text = fs::read_to_string(entry.path()).unwrap();
            // First make sure the source is Linux-style, then convert, so we never get \r\r\n.
            let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
            fs::write(target, text).unwrap();
        }
    }
}

fn glued_sample() -> String {
    bundle(&sample_dir()).expect("the sample tree should glue")
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// Index of the first line that is exactly `wanted`.
fn position_of(output: &str, wanted: &str) -> usize {
    lines(output)
        .iter()
        .position(|line| *line == wanted)
        .unwrap_or_else(|| panic!("no line {wanted:?} in the output:\n{output}"))
}

/// A tiny project laid out like the real repo, for the `run` tests (test 8).
fn make_project(name: &str, welcome_text: &str) -> PathBuf {
    make_tree(
        name,
        &[
            (
                &format!("{SOURCE_DIR}/lib.rs"),
                "//! Tiny crate.\npub mod welcome;\n",
            ),
            (&format!("{SOURCE_DIR}/welcome.rs"), welcome_text),
        ],
    )
}

const WELCOME_V1: &str = "pub const MESSAGE: &str = \"hi\";\n";

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

// ---------------------------------------------------------------- 1. same input, same output

#[test]
fn same_input_twice_gives_identical_output() {
    assert_eq!(glued_sample(), glued_sample());
}

// ---------------------------------------------------------------- 2. line endings

#[test]
fn windows_line_endings_give_the_same_output_as_linux_ones() {
    let windows_copy = Path::new(env!("CARGO_TARGET_TMPDIR")).join("park-bundle-crlf-sample");
    let _ = fs::remove_dir_all(&windows_copy);
    copy_with_windows_line_endings(&sample_dir(), &windows_copy);

    let from_windows = bundle(&windows_copy).unwrap();
    assert_eq!(from_windows, glued_sample());
    assert!(
        !from_windows.contains('\r'),
        "output must use Linux line endings"
    );
}

// ---------------------------------------------------------------- 3. nesting, pub, attributes

#[test]
fn modules_nest_and_keep_pub_and_attributes() {
    let out = glued_sample();

    // `mod a;` -> a.rs, `pub mod b;` -> b/mod.rs, and `mod c;` inside b -> b/c.rs.
    let a = position_of(&out, "mod a {");
    let b = position_of(&out, "pub mod b {");
    let c = position_of(&out, "mod c {");
    assert!(a < b && b < c, "modules should appear in order a, b, c");

    // Text from each file really landed inside its module.
    assert!(out.contains("// Module a: a plain private module, found at a.rs."));
    assert!(out.contains("// Module c: a child of b, found at b/c.rs."));
    assert!(out.contains("    \"hello\""));

    // Attributes and docs written above a `mod` line stay directly above it.
    assert_eq!(lines(&out)[c - 1], "#[allow(dead_code)]");
    assert_eq!(lines(&out)[b - 1], "/// Documentation on a public module.");
    // Inline test blocks are kept.
    assert_eq!(
        lines(&out)[position_of(&out, "mod tests {") - 1],
        "#[cfg(test)]"
    );
}

// ---------------------------------------------------------------- 4. crate:: paths

#[test]
fn crate_paths_get_the_game_prefix_but_other_crates_do_not() {
    let out = glued_sample();
    assert_eq!(GAME_PREFIX, "crate::game::htp_park::");

    assert!(out.contains("    crate::game::htp_park::a::hello()"));
    assert!(out.contains("    use crate::game::htp_park::a::hello;"));
    assert!(out.contains("    crate::game::htp_park::b::c::twice(x)"));
    // `my_crate::` and `$crate::` are different words and stay as they are.
    assert!(out.contains("    my_crate::something();"));
    assert!(out.contains("        $crate::b::double($x)"));
}

// ---------------------------------------------------------------- 5. //! comments

#[test]
fn inner_doc_comments_become_plain_comments() {
    let out = glued_sample();
    assert!(
        !out.lines().any(|line| line.trim_start().starts_with("//!")),
        "no //! should be left"
    );
    assert!(out.contains("// The sample crate's first doc line."));
    assert!(out.contains("//\n"), "an empty //! line becomes a bare //");
    assert!(out.contains("// Module b: found at b/mod.rs, and it has a child module of its own."));
}

// ---------------------------------------------------------------- 6. refused items

/// Glue a crate whose third line is `offending`, and return the error.
fn refused_error(case: &str, offending: &str) -> BundleError {
    let source = format!("// fine\npub fn ok() {{}}\n{offending}\n");
    let dir = make_tree(case, &[("lib.rs", &source)]);
    bundle(&dir).expect_err("this line should have been refused")
}

#[test]
fn forbidden_items_are_refused_with_file_and_line() {
    // (a name for the temp folder, the offending line, what the message must call it)
    let cases = [
        ("attr", "#![allow(dead_code)]", "#![…]"),
        ("path", "#[path = \"x.rs\"]", "#[path"),
        ("include", "include!(\"x.rs\");", "`include!`"),
        (
            "include-str",
            "const A: &str = include_str!(\"x\");",
            "`include_str!`",
        ),
        (
            "include-bytes",
            "const A: &[u8] = include_bytes!(\"x\");",
            "`include_bytes!`",
        ),
        (
            "env",
            "const A: &str = env!(\"CARGO_PKG_VERSION\");",
            "`env!`",
        ),
        (
            "option-env",
            "const A: Option<&str> = option_env!(\"X\");",
            "`option_env!`",
        ),
        ("macro-export", "#[macro_export]", "`#[macro_export]`"),
        ("extern-crate", "extern crate alloc;", "`extern crate`"),
    ];
    for (case, offending, label) in cases {
        let error = refused_error(&format!("refuse-{case}"), offending);
        match &error {
            BundleError::Refused { file, line, .. } => {
                assert_eq!(*line, 3, "{case}: wrong line");
                assert!(file.ends_with("lib.rs"), "{case}: wrong file {file:?}");
            }
            other => panic!("{case}: expected a refusal, got {other:?}"),
        }
        let message = error.to_string();
        assert!(message.contains("lib.rs:3:"), "{case}: {message}");
        assert!(message.contains(label), "{case}: {message}");
    }
}

#[test]
fn a_refusal_names_the_module_file_it_is_in() {
    let dir = make_tree(
        "refuse-in-module",
        &[
            ("lib.rs", "mod inner;\n"),
            ("inner.rs", "// one\n// two\n#![allow(unused)]\n"),
        ],
    );
    let error = bundle(&dir).unwrap_err();
    assert!(error.to_string().contains("inner.rs:3:"), "{error}");
}

#[test]
fn look_alikes_and_comments_are_not_refused() {
    let dir = make_tree(
        "lookalikes",
        &[(
            "lib.rs",
            "// env!(\"X\") and #![x] and include!(\"y\") in a comment\n\
             /// extern crate in a doc comment\n\
             pub fn my_env_name() {}\n\
             pub fn f() { my_env!(1); my_include!(2); }\n",
        )],
    );
    assert!(bundle(&dir).is_ok());
}

#[test]
fn a_missing_module_file_is_reported() {
    let dir = make_tree(
        "missing-module",
        &[("lib.rs", "pub fn x() {}\nmod ghost;\n")],
    );
    let error = bundle(&dir).unwrap_err();
    assert!(
        matches!(error, BundleError::MissingModule { line: 2, .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("lib.rs:2:") && message.contains("ghost"),
        "{message}"
    );
}

#[test]
fn both_x_rs_and_x_mod_rs_is_reported() {
    let dir = make_tree(
        "ambiguous-module",
        &[
            ("lib.rs", "mod x;\n"),
            ("x.rs", "// one\n"),
            ("x/mod.rs", "// two\n"),
        ],
    );
    let error = bundle(&dir).unwrap_err();
    assert!(
        matches!(error, BundleError::AmbiguousModule { line: 1, .. }),
        "{error:?}"
    );
}

// ---------------------------------------------------------------- 7. the output is clean

#[test]
fn the_output_has_no_refused_items_and_no_bare_crate_paths() {
    let out = glued_sample();

    for (index, line) in out.lines().enumerate() {
        assert!(
            find_refusal(line).is_none(),
            "line {} of the output is refused: {line}",
            index + 1
        );
    }

    // Every `crate::` is either the new prefix, or part of another word (`$crate::`, `my_crate::`).
    for (at, _) in out.match_indices("crate::") {
        let before = out[..at].chars().next_back();
        let part_of_another_word =
            before.is_some_and(|c| c == '$' || c == '_' || c.is_alphanumeric());
        let is_new_prefix = out[at..].starts_with(GAME_PREFIX);
        assert!(
            part_of_another_word || is_new_prefix,
            "a bare crate:: path is left at byte {at}"
        );
    }
}

// ---------------------------------------------------------------- 8. --check

#[test]
fn check_says_fresh_right_after_gluing() {
    let root = make_project("check-fresh", WELCOME_V1);
    assert_eq!(run(&[], &root).code, 0);
    assert!(root.join(OUTPUT_FILE).is_file());

    let outcome = run(&args(&["--check"]), &root);
    assert_eq!(outcome.code, 0, "{}", outcome.message);
}

#[test]
fn check_says_stale_after_a_one_character_change() {
    let root = make_project("check-stale", WELCOME_V1);
    assert_eq!(run(&[], &root).code, 0);

    // Change "hi" to "ho" in the generated file.
    let path = root.join(OUTPUT_FILE);
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("\"hi\"", "\"ho\"");
    fs::write(&path, edited).unwrap();

    let outcome = run(&args(&["--check"]), &root);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.message.contains("park.rs is stale"),
        "{}",
        outcome.message
    );
    assert!(
        outcome.message.contains("cargo run -p park-bundle"),
        "{}",
        outcome.message
    );
}

#[test]
fn check_says_stale_when_the_source_changes_but_park_rs_does_not() {
    let root = make_project("check-source-changed", WELCOME_V1);
    assert_eq!(run(&[], &root).code, 0);

    fs::write(
        root.join(SOURCE_DIR).join("welcome.rs"),
        "pub const MESSAGE: &str = \"hello\";\n",
    )
    .unwrap();
    assert_eq!(run(&args(&["--check"]), &root).code, 1);

    // Gluing again makes it fresh again.
    assert_eq!(run(&[], &root).code, 0);
    assert_eq!(run(&args(&["--check"]), &root).code, 0);
}

#[test]
fn check_ignores_line_ending_style() {
    let root = make_project("check-crlf", WELCOME_V1);
    assert_eq!(run(&[], &root).code, 0);

    // Rewrite the generated file with Windows line endings: same text, different invisible marks.
    let path = root.join(OUTPUT_FILE);
    let windows_style = fs::read_to_string(&path).unwrap().replace('\n', "\r\n");
    fs::write(&path, windows_style).unwrap();

    let outcome = run(&args(&["--check"]), &root);
    assert_eq!(outcome.code, 0, "{}", outcome.message);
}

#[test]
fn check_says_stale_when_park_rs_is_missing() {
    let root = make_project("check-missing", WELCOME_V1);
    assert_eq!(run(&args(&["--check"]), &root).code, 1);
}

#[test]
fn errors_show_repo_relative_paths_and_exit_code_two() {
    let root = make_tree(
        "run-error",
        &[(&format!("{SOURCE_DIR}/lib.rs"), "// ok\n#![allow(x)]\n")],
    );
    let outcome = run(&[], &root);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .message
            .starts_with(&format!("{SOURCE_DIR}/lib.rs:2:")),
        "{}",
        outcome.message
    );
}

#[test]
fn unknown_arguments_are_a_usage_error() {
    let root = make_project("usage", WELCOME_V1);
    assert_eq!(run(&args(&["--nope"]), &root).code, 2);
}

// ---------------------------------------------------------------- 9. the mod's toml matches the prefix

fn mod_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(OUTPUT_FILE)
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The text after `key =` on a line, without the quotes (`name = "x"` gives `x`).
fn toml_value(line: &str, key: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(key)?.trim_start();
    let value = rest.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
}

/// The `name` of the `[[module]]` block whose `source` is `source`.
fn module_name_for_source(toml: &str, source: &str) -> Option<String> {
    let mut found = None;
    let mut name = None;
    let mut this_source = None;
    // A line starting with `[` begins a new block, so the block before it is complete.
    for line in toml.lines().chain(std::iter::once("[end]")) {
        if line.trim_start().starts_with('[') {
            if this_source.as_deref() == Some(source) {
                found = name.take();
            }
            name = None;
            this_source = None;
        }
        name = toml_value(line, "name").or(name);
        this_source = toml_value(line, "source").or(this_source);
    }
    found
}

#[test]
fn the_mixin_toml_names_the_module_that_the_prefix_points_at() {
    let toml = fs::read_to_string(mod_folder().join("horizon-mixin.toml")).unwrap();
    let park_file = Path::new(OUTPUT_FILE)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();

    // "crate::game::htp_park::" -> the module is called "htp_park".
    let expected = GAME_PREFIX
        .strip_prefix("crate::game::")
        .unwrap()
        .trim_end_matches("::");
    assert_eq!(
        module_name_for_source(&toml, park_file).as_deref(),
        Some(expected),
        "horizon-mixin.toml must install park.rs as the module the prefix points at"
    );
    assert_eq!(expected, "htp_park");
}

#[test]
fn the_pinned_horizon_commit_is_the_same_in_both_files() {
    let folder = mod_folder();
    let toml = fs::read_to_string(folder.join("horizon-mixin.toml")).unwrap();
    let json = fs::read_to_string(folder.join("../../compatibility.json")).unwrap();

    let pin_in_toml = toml
        .lines()
        .find_map(|line| toml_value(line, "horizon_revision"))
        .expect("horizon-mixin.toml has a horizon_revision");
    assert_eq!(pin_in_toml.len(), 40, "a full commit id is 40 characters");
    assert!(
        json.contains(&pin_in_toml),
        "compatibility.json has a different pin"
    );
}
