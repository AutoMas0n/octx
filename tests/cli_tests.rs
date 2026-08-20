use octx::{CliArgs, run};

#[test]
fn test_run_returns_greeting() {
    let args = CliArgs {
        name: Some("World".into()),
    };
    let result = run(&args).unwrap();
    assert_eq!(result, "Hello from octx library!");
}
