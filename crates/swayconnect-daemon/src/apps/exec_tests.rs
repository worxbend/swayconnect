use super::*;

fn argv(raw: &str) -> Vec<String> {
    parse_exec(raw)
        .unwrap_or_else(|error| panic!("{raw:?}: {error}"))
        .argv
}

#[test]
fn plain_words_split_on_spaces() {
    assert_eq!(argv("vim  -p"), ["vim", "-p"]);
    assert_eq!(
        argv("/usr/bin/mpv --player-operation-mode=pseudo-gui --"),
        ["/usr/bin/mpv", "--player-operation-mode=pseudo-gui", "--"]
    );
}

#[test]
fn quoted_arguments_keep_spaces_and_reserved_characters() {
    assert_eq!(argv(r#""/opt/My App/run" %f"#), ["/opt/My App/run"]);
    assert_eq!(
        argv(r#"sh -c "echo a; echo b | cat""#),
        ["sh", "-c", "echo a; echo b | cat"]
    );
    assert_eq!(argv(r#"app """#), ["app", ""]);
}

#[test]
fn escapes_inside_quotes_follow_the_spec() {
    // The string escape runs first, so `\\"` and `\"` both reach the quoting rule as `\"`.
    assert_eq!(argv(r#"app "say \"hi\"""#), ["app", r#"say "hi""#]);
    assert_eq!(argv(r#"app "say \\"hi\\"""#), ["app", r#"say "hi""#]);
    // Four backslashes are one literal backslash; `\\$` is one literal dollar sign.
    assert_eq!(argv(r#"app "C:\\\\dir""#), ["app", r"C:\dir"]);
    assert_eq!(argv(r#"app "\\$HOME" "\\`x\\`""#), ["app", "$HOME", "`x`"]);
}

#[test]
fn string_escapes_apply_before_splitting() {
    assert_eq!(argv(r"app a\sb"), ["app", "a", "b"]);
    assert_eq!(argv(r#"app "a\sb""#), ["app", "a b"]);
    assert_eq!(parse_exec(r"app a\nb"), Err(ExecError::Reserved('\n')));
}

#[test]
fn every_field_code_is_removed() {
    for code in [
        "%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%i", "%c", "%k", "%v", "%m",
    ] {
        assert_eq!(
            argv(&format!("app {code} --flag")),
            ["app", "--flag"],
            "{code}"
        );
    }
    assert_eq!(argv("app --file=%f"), ["app", "--file="]);
    assert_eq!(argv(r#"app "%u""#), ["app"]);
    assert_eq!(argv("app 100%%"), ["app", "100%"]);
}

#[test]
fn url_field_codes_are_reported() {
    assert!(parse_exec("firefox %u").expect("valid").takes_urls);
    assert!(
        parse_exec("nautilus --new-window %U")
            .expect("valid")
            .takes_urls
    );
    assert!(!parse_exec("mpv %F").expect("valid").takes_urls);
}

#[test]
fn a_real_url_handler_line_parses() {
    let line = parse_exec(
        r#"kde-geo-uri-handler --coordinate-template "https://example.org/?geo=<URI>" --fallback "https://example.org" %u"#,
    )
    .expect("valid");
    assert_eq!(
        line.argv,
        [
            "kde-geo-uri-handler",
            "--coordinate-template",
            "https://example.org/?geo=<URI>",
            "--fallback",
            "https://example.org"
        ]
    );
    assert!(line.takes_urls);
}

#[test]
fn malformed_lines_are_refused() {
    let cases = [
        ("", ExecError::Empty),
        ("   ", ExecError::Empty),
        ("%U", ExecError::Empty),
        (r#""" --x"#, ExecError::Empty),
        (r#"app "open"#, ExecError::UnterminatedQuote),
        (r#"app a"b""#, ExecError::MisplacedQuote),
        (r#"app "a"b"#, ExecError::MisplacedQuote),
        (r#"app "a"%f"#, ExecError::MisplacedQuote),
        (r#"app "a\qb""#, ExecError::BadEscape),
        ("app %z", ExecError::UnknownFieldCode),
        ("app %", ExecError::UnknownFieldCode),
        ("sh -c id;reboot", ExecError::Reserved(';')),
        ("app $HOME", ExecError::Reserved('$')),
        ("app 'one two'", ExecError::Reserved('\'')),
        ("app ~/x", ExecError::Reserved('~')),
        ("app a>b", ExecError::Reserved('>')),
        (r"app a\\b", ExecError::Reserved('\\')),
        ("app\targ", ExecError::Reserved('\t')),
    ];
    for (raw, expected) in cases {
        assert_eq!(parse_exec(raw), Err(expected), "{raw:?}");
    }
}

#[test]
fn value_and_list_escapes() {
    assert_eq!(unescape_value(r"a\sb\tc\\d\qe"), "a b\tc\\d\\qe");
    assert_eq!(unescape_value("trailing\\"), "trailing\\");
    assert_eq!(split_list(r"web;browser\;tool;;"), ["web", "browser;tool"]);
    assert_eq!(split_list(""), Vec::<String>::new());
}
