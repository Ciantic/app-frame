//! What the handles refuse to compile.

#[test]
fn undeclared_operators_do_not_compile() {
    let cases = trybuild::TestCases::new();

    cases.compile_fail("tests/ui/fail_*.rs");
}
