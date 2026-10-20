use std::env;

use super::*;

/// A fresh folder under the temp dir.
fn folder(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!("elyterminal-{name}-{}", std::process::id()));
    if folder.exists() {
        fs::remove_dir_all(&folder).expect("a stale test folder goes");
    }
    fs::create_dir_all(&folder).expect("a test folder");
    folder
}

#[test]
fn folders_come_first_then_files_by_name_ignoring_case() {
    let root = folder("files-list");
    for name in ["Zeta", "alpha"] {
        fs::create_dir(root.join(name)).expect("a folder");
    }
    for name in ["c.txt", "B.txt", "a.txt"] {
        fs::write(root.join(name), "").expect("a file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(root.join("Zeta"), root.join("link")).expect("a link to a folder");
        symlink(root.join("nowhere"), root.join("broken")).expect("a broken link");
    }
    let expected: &[(&str, bool)] = if cfg!(unix) {
        &[
            ("alpha", true),
            ("link", true),
            ("Zeta", true),
            ("a.txt", false),
            ("B.txt", false),
            ("broken", false),
            ("c.txt", false),
        ]
    } else {
        &[
            ("alpha", true),
            ("Zeta", true),
            ("a.txt", false),
            ("B.txt", false),
            ("c.txt", false),
        ]
    };
    let listed: Vec<_> = list(&root).expect("a listing");
    let listed: Vec<_> = listed
        .iter()
        .map(|entry| (entry.name.to_str().expect("a UTF-8 name"), entry.folder))
        .collect();
    assert_eq!(listed, expected);
    assert!(list(&root.join("missing")).is_err());
}

#[test]
fn a_mark_dots_each_folder_above_it_up_to_the_root() {
    let root = Path::new("/r");
    let changes = vec![
        (
            root.join("src").join("ui").join("a.rs"),
            GitStatus::Modified,
        ),
        (root.join("gone.txt"), GitStatus::Deleted),
    ];
    let (marks, dirty) = marks(root, changes);
    assert_eq!(marks.len(), 2);
    let expected = HashSet::from([
        root.to_path_buf(),
        root.join("src"),
        root.join("src").join("ui"),
    ]);
    assert_eq!(dirty, expected);
}

#[test]
fn the_tree_wears_marks_and_an_untracked_folder_marks_what_it_holds() {
    let root = PathBuf::from("/r");
    let (notes, src, locked) = (root.join("notes"), root.join("src"), root.join("locked"));
    let entry = |name: &str, folder| Entry {
        name: name.into(),
        folder,
    };
    let mut files = Files::new(root.clone());
    let top = vec![
        entry("notes", true),
        entry("src", true),
        entry("locked", true),
        entry("a.txt", false),
    ];
    files.listings.insert(root.clone(), Ok(top));
    let inside = vec![entry("deep", true), entry("todo.txt", false)];
    files.listings.insert(notes.clone(), Ok(inside));
    files
        .listings
        .insert(src.clone(), Ok(vec![entry("main.rs", false)]));
    files.listings.insert(locked.clone(), Err("denied".into()));
    let changes = vec![
        (notes.clone(), GitStatus::Untracked),
        (src.join("main.rs"), GitStatus::Modified),
    ];
    (files.marks, files.dirty) = marks(&root, changes);

    let node = |path: &Path, icon| {
        let label = path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .to_string();
        TreeNode::new(key(path), path.to_path_buf(), label).icon(icon)
    };
    let untracked = |node: TreeNode| node.note("U").tone(Tone::Success);
    let dotted = |node: TreeNode| node.note("•").tone(Tone::Warning);
    let expected = dotted(
        node(&root, IconName::Folder).children([
            untracked(node(&notes, IconName::Folder).children([
                untracked(node(&notes.join("deep"), IconName::Folder).pending()),
                untracked(node(&notes.join("todo.txt"), IconName::File)),
            ])),
            dotted(
                node(&src, IconName::Folder).children([node(&src.join("main.rs"), IconName::File)
                    .note("M")
                    .tone(Tone::Warning)]),
            ),
            node(&locked, IconName::Folder)
                .children([])
                .note("unreadable")
                .tone(Tone::Danger),
            node(&root.join("a.txt"), IconName::File),
        ]),
    );
    assert_eq!(files.folder(&root, "r".into(), false), expected);
}

#[test]
fn names_with_newlines_label_on_one_line_and_keep_their_paths() {
    let root = PathBuf::from("/r");
    let (folder, file) = (root.join("line\nbreak"), root.join("report\n2026.txt"));
    let mut files = Files::new(root.clone());
    let entries = vec![
        Entry {
            name: "line\nbreak".into(),
            folder: true,
        },
        Entry {
            name: "report\n2026.txt".into(),
            folder: false,
        },
    ];
    files.listings.insert(root.clone(), Ok(entries));
    let expected = TreeNode::new(key(&root), root.clone(), "r")
        .icon(IconName::Folder)
        .children([
            TreeNode::new(key(&folder), folder.clone(), "line␊break")
                .icon(IconName::Folder)
                .pending(),
            TreeNode::new(key(&file), file.clone(), "report␊2026.txt").icon(IconName::File),
        ]);
    assert_eq!(files.folder(&root, "r".into(), false), expected);
}

#[cfg(unix)]
#[test]
fn names_that_show_alike_keep_their_own_keys_and_paths() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let root = PathBuf::from("/r");
    let raw = OsStr::from_bytes(b"bad\xff.txt");
    let (text, bytes) = (root.join("bad\u{FFFD}.txt"), root.join(raw));
    let mut files = Files::new(root.clone());
    let entry = |name: &OsStr| Entry {
        name: name.into(),
        folder: false,
    };
    let entries = vec![entry(OsStr::new("bad\u{FFFD}.txt")), entry(raw)];
    files.listings.insert(root.clone(), Ok(entries));
    assert_ne!(key(&text), key(&bytes), "two paths, two keys");
    let expected = TreeNode::new(key(&root), root.clone(), "r")
        .icon(IconName::Folder)
        .children([
            TreeNode::new(key(&text), text.clone(), "bad\u{FFFD}.txt").icon(IconName::File),
            TreeNode::new(key(&bytes), bytes.clone(), "bad\u{FFFD}.txt").icon(IconName::File),
        ]);
    assert_eq!(files.folder(&root, "r".into(), false), expected);
    for path in [&root, &text, &bytes] {
        let found = files.path(&key(path));
        assert_eq!(found.as_ref(), Some(path), "{}", path.display());
    }
    assert_eq!(files.path("/r/gone"), None);
}

/// Runs git in `root` as a fixed author, unsigned.
#[cfg(target_os = "macos")]
fn run(root: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=Ely", "-c", "user.email=ely@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    let error = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "git {args:?}: {error}");
}

#[cfg(target_os = "macos")]
#[test]
fn decomposed_names_on_disk_wear_the_composed_marks_git_writes() {
    let root = folder("files-nfd");
    run(&root, &["init", "-q"]);
    run(&root, &["config", "core.precomposeunicode", "true"]);
    run(&root, &["config", "status.showUntrackedFiles", "normal"]);
    let (dossier, tracked, untracked) = (
        "dossier-e\u{301}",
        "tracked-e\u{301}.txt",
        "new-e\u{301}.txt",
    );
    fs::create_dir(root.join(dossier)).expect("a folder");
    fs::write(root.join(dossier).join("inner.txt"), "first").expect("a file");
    fs::write(root.join(tracked), "first").expect("a file");
    run(&root, &["add", "."]);
    run(&root, &["commit", "-qm", "first"]);
    fs::write(root.join(dossier).join("inner.txt"), "second").expect("an edit");
    fs::write(root.join(tracked), "second").expect("an edit");
    fs::write(root.join(untracked), "new").expect("an untracked file");

    let mut files = Files::new(root.clone());
    files.listings.insert(root.clone(), list(&root));
    let changes = git::changes(&root).expect("status").expect("a work tree");
    (files.marks, files.dirty) = marks(&root, changes);
    let node =
        |name: &str, icon| TreeNode::new(key(&root.join(name)), root.join(name), name).icon(icon);
    let dotted = |node: TreeNode| node.note("•").tone(Tone::Warning);
    let expected = dotted(
        TreeNode::new(key(&root), root.clone(), "r")
            .icon(IconName::Folder)
            .children([
                node(".git", IconName::Folder).pending(),
                dotted(node(dossier, IconName::Folder).pending()),
                node(untracked, IconName::File)
                    .note("U")
                    .tone(Tone::Success),
                node(tracked, IconName::File).note("M").tone(Tone::Warning),
            ]),
    );
    assert_eq!(files.folder(&root, "r".into(), false), expected);
}

#[test]
fn a_dragged_file_targets_the_folder_dropped_into_or_the_sibling_parent() {
    let root = folder("files-move");
    let docs = root.join("docs");
    let src = root.join("src");
    fs::create_dir(&docs).expect("docs");
    fs::create_dir(&src).expect("src");
    let file = root.join("notes.txt");
    fs::write(&file, "hello").expect("file");

    // Dropping inside docs moves notes.txt into docs.
    let dest = move_destination(&file, &docs, DropAt::Inside);
    assert_eq!(dest, Some(docs.join("notes.txt")));

    // Dropping next to a file inside docs moves notes.txt into docs.
    let guide = docs.join("guide.md");
    let dest_sibling = move_destination(&file, &guide, DropAt::Before);
    assert_eq!(dest_sibling, Some(docs.join("notes.txt")));

    // Moving a folder into itself or a subfolder is refused.
    let sub = docs.join("sub");
    assert_eq!(move_destination(&docs, &docs, DropAt::Inside), None);
    assert_eq!(move_destination(&docs, &sub, DropAt::Inside), None);

    // Moving a file to where it already sits is refused.
    assert_eq!(move_destination(&file, &root, DropAt::Inside), None);
}

#[test]
fn tree_drag_holds_path_key_and_label() {
    let path = PathBuf::from("/home/user/test.txt");
    let drag = TreeDrag {
        owner: gpui::EntityId::from(1),
        path: path.clone(),
        key: "test.txt".into(),
        label: "test.txt".into(),
    };
    assert_eq!(drag.path, path);
    assert_eq!(drag.label, "test.txt");
}
