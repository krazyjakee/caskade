use crate::fixtures;
use caskade_installer::{
    ArtifactId, ArtifactNode, Candidate, Fidelity, Format, HandlerId, PayloadPlatform,
    Requirements, Role, RuntimeSpec, Step, Strategy,
    host::HostCaps,
    plan::{
        CandidateTransform, ExecutionPolicy, Handler, PlanContext, PlanEngine, PlanError, Policy,
        Proposal,
    },
    recipes::{InputIdentity, Recipe, RecipeError, RecipeSet},
    testing::TestRoot,
};
#[cfg(feature = "windows-formats")]
use caskade_installer::{
    exec::{CancellationToken, ExecRequest, Executor},
    testing::{FakeDiskSpace, FakeRuntime, RecordingSink},
};
use std::collections::BTreeMap;
#[cfg(feature = "windows-formats")]
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
fn node() -> ArtifactNode {
    ArtifactNode {
        id: ArtifactId("source:0".into()),
        path: "setup.exe".into(),
        format: Format::Pe,
        platform: PayloadPlatform::Windows,
        role: Role::Installer,
        size: Some(1),
        facts: BTreeMap::from([
            ("app_id".into(), "FixtureApp".into()),
            ("gog_id".into(), "123".into()),
        ]),
        children: vec![],
        children_complete: true,
    }
}
fn candidate(strategy: Strategy) -> Candidate {
    Candidate {
        handler: HandlerId("example".into()),
        strategy,
        confidence: 1.0,
        fidelity: Fidelity::Exact,
        requires: Requirements {
            executes_code: false,
            runtime: None,
            may_prompt_elevation: false,
            interactive: false,
            est_disk_bytes: None,
        },
        steps: vec![],
        warnings: vec![],
    }
}
fn recipe(id: &str, selectors: &str, fields: &str) -> Recipe {
    Recipe::parse(&format!(
        "schema_version=1\nid={id:?}\n{fields}\n[match]\n{selectors}\n"
    ))
    .unwrap()
}
#[test]
fn builtin_recipes_are_schema_valid_and_malformed_documents_are_rejected() {
    let recipes = RecipeSet::builtin().unwrap();
    assert_eq!(recipes.recipes().count(), 1);
    assert!(recipes.requires_hash());
    let schema = caskade_installer::recipes::json_schema();
    insta::assert_json_snapshot!("recipe_schema", schema);
    let validator = jsonschema::validator_for(&schema).unwrap();
    for recipe in recipes.recipes() {
        assert!(validator.is_valid(&serde_json::to_value(recipe).unwrap()));
    }
    for document in [
        "schema_version=2\nid='bad'\n[match]\ngog_id='123'",
        "schema_version=1\nid='bad'\n[match]",
        "schema_version=1\nid='bad'\n[match]\nsha256='notahash'",
        "schema_version=1\nid='bad'\nexecute_shell='ignored'\n[match]\ngog_id='123'",
    ] {
        assert!(Recipe::parse(document).is_err());
    }
    let mut invalid = recipe("bad", "gog_id='123'", "");
    invalid.id = "-bad".into();
    assert!(invalid.validate().is_err());
    let mut invalid = recipe("bad", "gog_id='123'", "");
    invalid.add_steps.push(Step::WriteFile {
        path: "../sentinel".into(),
        contents: b"bad".to_vec(),
    });
    assert!(invalid.validate().is_err());
}
#[test]
fn selectors_are_exact_and_combined_and_alias_metadata_is_exposed() {
    let mut set = RecipeSet::new();
    set.insert(recipe("combined","gog_id='123'\napp_id='FixtureApp'\nsha256='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'","strategy='extract'\ndisable_alias=true")).unwrap();
    let identity = InputIdentity {
        gog_id: Some("123".into()),
        app_id: Some("fixtureapp".into()),
        sha256: Some("A".repeat(64)),
    };
    let matched = set.resolve(&identity).unwrap();
    assert_eq!(matched.strategy, Some(Strategy::Extract));
    assert!(matched.disable_alias);
    assert_eq!(matched.recipe_ids, ["combined"]);
    for identity in [
        InputIdentity {
            gog_id: Some("124".into()),
            ..identity.clone()
        },
        InputIdentity {
            app_id: None,
            ..identity.clone()
        },
        InputIdentity {
            sha256: Some("b".repeat(64)),
            ..identity
        },
    ] {
        assert!(set.resolve(&identity).unwrap().recipe_ids.is_empty());
    }
    let node = node();
    let mut set = RecipeSet::new();
    set.insert(recipe("metadata", "app_id='FixtureApp'", ""))
        .unwrap();
    assert_eq!(
        set.resolve_artifact(&node).unwrap().recipe_ids,
        ["metadata"]
    );
}
#[test]
fn user_loading_is_deterministic_atomic_and_overrides_builtin_ids() {
    let root = TestRoot::new().unwrap();
    let mut set = RecipeSet::builtin().unwrap();
    std::fs::write(
        root.path().join("override.toml"),
        "schema_version=1\nid='fixture-inno-extract'\ndisable_alias=false\n[match]\ngog_id='123'\n",
    )
    .unwrap();
    set.load_directory(root.path()).unwrap();
    assert_eq!(set.recipes().count(), 1);
    assert!(!set.requires_hash());
    assert!(
        !set.resolve(&InputIdentity {
            gog_id: Some("123".into()),
            ..Default::default()
        })
        .unwrap()
        .disable_alias
    );
    let before = set.recipes().cloned().collect::<Vec<_>>();
    std::fs::write(root.path().join("bad.toml"), "invalid !!!").unwrap();
    assert!(set.load_directory(root.path()).is_err());
    assert_eq!(set.recipes().cloned().collect::<Vec<_>>(), before);
    std::fs::remove_file(root.path().join("bad.toml")).unwrap();
    std::fs::copy(
        root.path().join("override.toml"),
        root.path().join("duplicate.toml"),
    )
    .unwrap();
    assert!(set.load_directory(root.path()).is_err());
    root.assert_sentinel_untouched();
}
#[test]
fn distinct_matching_recipe_conflicts_fail_closed() {
    let mut set = RecipeSet::new();
    set.insert(recipe("first", "gog_id='123'", "strategy='extract'"))
        .unwrap();
    set.insert(recipe(
        "second",
        "app_id='FixtureApp'",
        "strategy='run_silent'",
    ))
    .unwrap();
    assert!(matches!(
        set.resolve_artifact(&node()),
        Err(RecipeError::Conflict(_))
    ));
}
struct ExampleHandler;
impl Handler for ExampleHandler {
    fn id(&self) -> HandlerId {
        HandlerId("example".into())
    }
    fn claims(&self, _: &ArtifactNode) -> bool {
        true
    }
    fn propose(&self, _: &ArtifactNode, context: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        let mut candidate = candidate(Strategy::Extract);
        candidate.steps.push(Step::WriteFile {
            path: "preset.txt".into(),
            contents: format!(
                "{:?}:{}",
                context.policy.preset_args.language, context.policy.preset_args.no_shortcuts
            )
            .into_bytes(),
        });
        Ok(Proposal {
            candidates: vec![candidate],
            question: None,
        })
    }
}
#[test]
fn recipe_presets_are_artifact_scoped_and_added_processes_cannot_bypass_policy() {
    let mut set = RecipeSet::new();
    let mut recipe = recipe("presets", "app_id='FixtureApp'", "");
    recipe.preset_args.language = Some("german".into());
    recipe.preset_args.no_shortcuts = Some(false);
    recipe.add_steps.push(Step::Run {
        program: caskade_installer::Program::Tool("echo".into()),
        args: vec!["recipe".into()],
        env: BTreeMap::new(),
        runtime: RuntimeSpec::Native,
        timeout_secs: 10,
        expect: vec![],
    });
    set.insert(recipe).unwrap();
    let node = node();
    let changed = set.preset_args(&node, &Default::default()).unwrap();
    assert_eq!(changed.language.as_deref(), Some("german"));
    assert!(!changed.no_shortcuts);
    let mut planner = PlanEngine::new().with_transform(Box::new(set));
    planner.register(Box::new(ExampleHandler)).unwrap();
    let host: HostCaps = toml::from_str(include_str!("hosts/windows.toml")).unwrap();
    let policy = Policy {
        execution: ExecutionPolicy::Never,
        ..Policy::default()
    };
    assert!(
        planner
            .plan(std::slice::from_ref(&node), &host, &policy)
            .unwrap()
            .candidates
            .is_empty()
    );
    let plans = planner.plan(&[node], &host, &Policy::default()).unwrap();
    assert!(plans.candidates[0].requires.executes_code);
    assert_eq!(Policy::default().preset_args.language, None);
}
#[cfg(feature = "windows-formats")]
#[tokio::test]
async fn committed_inno_recipe_forces_extraction_and_write_file_through_executor() {
    use caskade_installer::{
        formats::windows::inno::{InnoExtractor, InnoHandler},
        probe::{ProbeEngine, ProbeLimits},
    };
    let fixture =
        fixtures::load_windows(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
            .unwrap()
            .into_iter()
            .find(|f| f.expected.id == "inno60-basic")
            .unwrap();
    let input = fixture.input();
    let node = ProbeEngine::with_builtin_formats(ProbeLimits::default())
        .probe_path(&input)
        .unwrap()
        .root;
    let host: HostCaps = toml::from_str(include_str!("hosts/windows.toml")).unwrap();
    let target = Path::new("C:/Games/Recipe Fixture");
    let policy = Policy::default();
    let context = PlanContext::new(&host, &policy).with_target_dir(target);
    let mut ordinary = PlanEngine::new();
    ordinary.register(Box::new(InnoHandler::new())).unwrap();
    assert_eq!(
        ordinary
            .plan_with_context(std::slice::from_ref(&node), &context)
            .unwrap()
            .candidates[0]
            .strategy,
        Strategy::RunSilent
    );
    let mut recipes = RecipeSet::load(None).unwrap();
    recipes.bind_input(&node, &input).unwrap();
    let resolved = recipes.resolve_artifact(&node).unwrap();
    assert_eq!(resolved.recipe_ids, ["fixture-inno-extract"]);
    assert!(resolved.disable_alias);
    let mut planner = PlanEngine::new().with_transform(Box::new(recipes));
    planner.register(Box::new(InnoHandler::new())).unwrap();
    let plans = planner
        .plan_with_context(std::slice::from_ref(&node), &context)
        .unwrap();
    assert!(plans.question.is_none());
    assert_eq!(plans.candidates[0].strategy, Strategy::Extract);
    assert_eq!(plans.candidates.len(), 1);
    let sources = BTreeMap::from([(node.id.clone(), input)]);
    let runtime = Arc::new(FakeRuntime::default());
    let extractor = Arc::new(InnoExtractor::new(sources.clone()));
    let root = TestRoot::new().unwrap();
    let report = Executor::new(
        runtime.clone(),
        Arc::new(FakeDiskSpace::fixed(u64::MAX)),
        Arc::new(RecordingSink::default()),
    )
    .with_extractor(extractor)
    .execute(
        ExecRequest {
            job_id: "recipe-fixture".into(),
            root: root.path().into(),
            relative_dir: PathBuf::from("Recipe Fixture"),
            plans,
            sources,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(report.dir.join("recipe-applied.txt")).unwrap(),
        b"recipe applied\n"
    );
    std::fs::remove_file(report.dir.join("recipe-applied.txt")).unwrap();
    fixture.assert_tree(&report.dir).unwrap();
    assert!(runtime.processes().is_empty());
    root.assert_sentinel_untouched();
}

#[test]
fn recipe_loading_limits_and_symlinks_fail_without_external_reads_or_writes() {
    let root = TestRoot::new().unwrap();
    let huge = "x".repeat(caskade_installer::recipes::MAX_RECIPE_BYTES + 1);
    assert!(matches!(Recipe::parse(&huge), Err(RecipeError::Limit)));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.sentinel_path(), root.path().join("escape.toml")).unwrap();
        assert!(matches!(
            RecipeSet::load(Some(root.path())),
            Err(RecipeError::Invalid(_))
        ));
    }
    root.assert_sentinel_untouched();
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(256))]
    #[test]
    fn arbitrary_recipe_text_is_rejected_or_validated(text in ".{0,2048}") {
        if let Ok(recipe) = Recipe::parse(&text) { proptest::prop_assert!(recipe.validate().is_ok()); }
    }
}
