# Architecture

High-level map of how `ruby-fast-cop` is wired. Complements `CLAUDE.md` (which covers conventions) and `COPS.md` (status).

## System overview

End-to-end flow from a file on disk to offenses (optionally autocorrected) on stdout.

```mermaid
flowchart TD
    CLI["main.rs (CLI)"] --> Runner["runner — parallel file walk (rayon)"]
    Config[".rubocop.yml"] --> CfgParser["config::Config (serde_yaml)"]
    CfgParser --> Runner

    Runner --> Lib["lib::check_and_correct_file"]
    Lib --> Build["registry::build_from_config"]
    Build -->|"inventory::iter"| Registry[("Registration entries<br/>one per cop, link-time collected")]
    Build --> Cops["Vec&lt;Box&lt;dyn Cop&gt;&gt;"]

    Lib --> Parse["ruby_prism::parse → ParseResult"]
    Parse --> CopRunner["cops::CopRunner (single AST walk)"]
    Cops --> CopRunner
    CopRunner --> Offenses["Vec&lt;Offense&gt;"]

    Offenses -->|"-a / -A"| Correct["correction::apply_corrections_detailed<br/>(iterative, up to 10 passes)"]
    Correct --> Lib

    Offenses --> Fmt["formatters (progress / json / emacs)"]
    Fmt --> Stdout["stdout / --out file"]
```

Key points:
- **Single parse per file** — Prism runs once, all cops share the `ParseResult`.
- **Single AST walk per file** — `CopRunner` implements `ruby_prism::Visit` and fans each node out to every registered cop's relevant `check_*` method. No per-cop traversal.
- **Autocorrect is a fixpoint loop** — parse → lint → apply → repeat until stable, cycle-detected, or 10 iterations (Ruff model, not RuboCop's 200).
- **Parallelism is at file granularity** — rayon splits files across threads; a single file is checked serially.

## Cop implementation architecture

How one cop goes from source file to being invoked during an AST walk.

```mermaid
flowchart LR
    subgraph File["src/cops/{dept}/foo.rs"]
        Impl["struct Foo { ... }<br/>impl Cop for Foo"]
        Reg["register_cop!(&quot;Dept/Foo&quot;, factory)"]
    end

    Reg -->|"inventory::submit!"| Inv[("link-time registry")]

    subgraph Runtime["Runtime (per file)"]
        Cfg["&Config"] --> BuildFn["registry::build_from_config"]
        Inv --> BuildFn
        BuildFn -->|"filter enabled → call factory"| Instances["Vec&lt;Box&lt;dyn Cop&gt;&gt;"]

        AST["Prism AST"] --> Walker["CopRunner::visit_*"]
        Instances --> Walker
        Walker -->|"dispatch by node kind"| CheckFns["cop.check_call / check_def / check_if / ..."]
        CheckFns -->|"&CheckContext"| Ctx[["source, filename,<br/>ruby_version,<br/>location helpers"]]
        CheckFns --> Off["Vec&lt;Offense&gt; (+ optional Correction)"]
    end
```

### The `Cop` trait (current surface)

Declared in `src/cops/mod.rs`. Each cop overrides only the `check_*` methods relevant to its node kinds — the rest default to empty `Vec<Offense>`. `CopRunner` dispatches once per node per cop during the shared walk.

Typical shapes:
- **Pattern cop** (`Style/RedundantFreeze`) — implements `check_call` only, matches method name, returns offense.
- **Scope-aware cop** (`Lint/UselessAssignment`) — implements `check_program`, spins up its own inner `Visit` walker with scope/branch state (uses `helpers::variable_force`).
- **Whole-file cop** (`Layout/LineLength`) — implements `check_program`, scans source by line rather than AST.

### Registration (`inventory`-backed)

```rust
// At the bottom of every cop file:
crate::register_cop!("Style/RedundantFreeze", |cfg| {
    let frozen_by_default = cfg.get_cop_config("Style/RedundantFreeze")
        .and_then(|c| c.raw.get("AllCopsStringLiteralsFrozenByDefault"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(Box::new(RedundantFreeze::with_config(frozen_by_default)))
});
```

`inventory::collect!(Registration)` in `src/cops/registry.rs` harvests every `submit!` at link time. `build_from_config` iterates, filters via `config.is_cop_enabled(name)`, and calls the factory. Three public entry points:

| Function | Use |
|---|---|
| `registry::build_from_config(&Config)` | Production — only enabled cops, config-applied |
| `registry::build_one(name, &Config)` | Test/--only harness — single cop |
| `registry::all_with_defaults()` | `cops::all()` — every cop with `Config::default()` |

Adding a cop touches exactly one file (`src/cops/{dept}/{name}.rs`) + one `pub use` in `{dept}/mod.rs` + flipping `implemented = true` in the TOML fixture. No `lib.rs` or `mod.rs::all()` edits.

## Shared infrastructure

Keep these mental models when reading/writing cops:

```mermaid
flowchart TD
    subgraph Helpers["src/helpers/"]
        Source["source.rs<br/>line/col, comment scan, chaining"]
        Escape["escape.rs<br/>string/regex escapes"]
        Access["access_modifier.rs"]
        Allowed["allowed_methods.rs<br/>(AllowedMethods/Patterns)"]
        VF["variable_force/<br/>scope → variable → assignment → branch"]
        NodeMatch["node_match.rs<br/>pattern predicates (m::is_*)"]
        CodeLen["code_length.rs"]
    end

    subgraph Core["src/"]
        Offense["offense.rs<br/>Offense / Location / Edit / Correction"]
        Correction["correction.rs<br/>apply_corrections (end-to-start, skip overlaps)"]
        Config["config/<br/>Config, CopConfig, inheritance"]
    end

    Helpers --> Cops["cops/*"]
    Offense --> Cops
    Correction --> Offense
    Config --> Cops
```

- **`CheckContext`** — the single thing every `check_*` method gets. Holds `source`, `filename`, `target_ruby_version`, and location helpers (`line_of`, `col_of`, `line_start`, `offense_with_range`). Mirrors RuboCop's `RangeHelp` / `Alignment` mixins.
- **`helpers::variable_force/`** — port of RuboCop's `Cop::VariableForce` (scope analyzer for useless/shadowed/unused assignment cops). Mirrors Ruby module file layout.
- **`helpers::node_match`** (`m::*` predicates) — small pattern helpers translating RuboCop's `def_node_matcher` patterns into Rust.

## Testing pipeline

```mermaid
flowchart LR
    RuboCop["RuboCop RSpec suite<br/>(cloned to /tmp/rubocop-repo)"] -->|"monkey-patch expect_offense"| Capture["test_data_capture.rb"]
    Capture --> Extract["extract_via_rspec.rb"]
    Extract --> Fixtures[("tests/fixtures/{dept}/{cop}.toml<br/>606 files, ~28k cases")]

    Fixtures --> Tester["tests/tester.rs"]
    Tester -->|"build cop via registry"| Impl["Rust cop impl"]
    Impl --> Result["Actual offenses + corrections"]
    Tester -->|"diff vs expected"| Report["pass / fail report"]
```

One TOML file per cop. `tester.rs` is cop-agnostic — it discovers fixtures, builds cops by name via `registry::build_one`, diffs offenses + corrected source.

## Where this document should be updated

Any time the following change, update this file:
- New top-level module in `src/` or department in `src/cops/`
- New trait/type in the public runtime surface (`Cop`, `CheckContext`, `Offense`, `Correction`)
- Registration mechanism (currently `inventory` + `register_cop!`)
- Autocorrect pipeline shape (iteration count, conflict strategy, entry points)
- Shared helper added to `src/helpers/` that multiple cops depend on
- Testing pipeline (fixture format, tester dispatch, extraction scripts)

If a change only touches a single cop's internals, it does **not** belong here — document it inline or in `CLAUDE.md`.
