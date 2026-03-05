# All 148 Prism Node Types (ruby-prism v1.9.0)

## Table of Contents

- [Literals](#literals)
- [Strings and Interpolation](#strings-and-interpolation)
- [Variables -- Read](#variables----read)
- [Variables -- Write](#variables----write)
- [Variables -- Compound Assignment](#variables----compound-assignment)
- [Variables -- Target (Multi-assign)](#variables----target-multi-assign)
- [Constants](#constants)
- [Control Flow](#control-flow)
- [Definitions](#definitions)
- [Calls and Blocks](#calls-and-blocks)
- [Operators](#operators)
- [Pattern Matching](#pattern-matching)
- [Parameters](#parameters)
- [Exceptions](#exceptions)

## Literals

- `IntegerNode` — `42`
- `FloatNode` — `3.14`
- `RationalNode` — `1/3r`
- `ImaginaryNode` — `2i`
- `TrueNode` — `true`
- `FalseNode` — `false`
- `NilNode` — `nil`
- `SelfNode` — `self`
- `SourceEncodingNode` — `__ENCODING__`
- `SourceFileNode` — `__FILE__`
- `SourceLineNode` — `__LINE__`
- `ArrayNode` — `[1, 2, 3]`
- `HashNode` — `{ a: 1 }`
- `KeywordHashNode` — `foo(a: 1)` (implicit hash in args)
- `RangeNode` — `1..10` / `1...10`
- `SymbolNode` — `:foo`
- `RegularExpressionNode` — `/pattern/`
- `XStringNode` — backtick `command`
- `MatchLastLineNode` — `if /regex/` (bare regex in condition)

## Strings and Interpolation

- `StringNode` — `'hello'` / `"hello"` (no interpolation)
- `InterpolatedStringNode` — `"hello #{name}"`
- `InterpolatedSymbolNode` — `:"hello_#{name}"`
- `InterpolatedRegularExpressionNode` — `/#{pattern}/`
- `InterpolatedXStringNode` — backtick `echo #{cmd}`
- `InterpolatedMatchLastLineNode` — `if /#{pattern}/`
- `EmbeddedStatementsNode` — `#{expr}` inside interpolation
- `EmbeddedVariableNode` — `#@var` / `#$var` inside string

## Variables -- Read

- `LocalVariableReadNode` — `x`
- `InstanceVariableReadNode` — `@x`
- `ClassVariableReadNode` — `@@x`
- `GlobalVariableReadNode` — `$x`
- `ConstantReadNode` — `Foo`
- `ConstantPathNode` — `Foo::Bar`
- `BackReferenceReadNode` — `$&`, `$~`
- `NumberedReferenceReadNode` — `$1`, `$2`
- `ItLocalVariableReadNode` — `it` (Ruby 3.4 anonymous block param)

## Variables -- Write

- `LocalVariableWriteNode` — `x = 1`
- `InstanceVariableWriteNode` — `@x = 1`
- `ClassVariableWriteNode` — `@@x = 1`
- `GlobalVariableWriteNode` — `$x = 1`
- `ConstantWriteNode` — `FOO = 1`
- `ConstantPathWriteNode` — `Foo::BAR = 1`
- `MultiWriteNode` — `a, b = 1, 2`

## Variables -- Compound Assignment

- `LocalVariableAndWriteNode` — `x &&= 1`
- `LocalVariableOrWriteNode` — `x ||= 1`
- `LocalVariableOperatorWriteNode` — `x += 1`
- `InstanceVariableAndWriteNode` — `@x &&= 1`
- `InstanceVariableOrWriteNode` — `@x ||= 1`
- `InstanceVariableOperatorWriteNode` — `@x += 1`
- `ClassVariableAndWriteNode` — `@@x &&= 1`
- `ClassVariableOrWriteNode` — `@@x ||= 1`
- `ClassVariableOperatorWriteNode` — `@@x += 1`
- `GlobalVariableAndWriteNode` — `$x &&= 1`
- `GlobalVariableOrWriteNode` — `$x ||= 1`
- `GlobalVariableOperatorWriteNode` — `$x += 1`
- `ConstantAndWriteNode` — `FOO &&= 1`
- `ConstantOrWriteNode` — `FOO ||= 1`
- `ConstantOperatorWriteNode` — `FOO += 1`
- `ConstantPathAndWriteNode` — `Foo::BAR &&= 1`
- `ConstantPathOrWriteNode` — `Foo::BAR ||= 1`
- `ConstantPathOperatorWriteNode` — `Foo::BAR += 1`
- `IndexAndWriteNode` — `a[0] &&= 1`
- `IndexOrWriteNode` — `a[0] ||= 1`
- `IndexOperatorWriteNode` — `a[0] += 1`
- `CallAndWriteNode` — `a.b &&= 1`
- `CallOrWriteNode` — `a.b ||= 1`
- `CallOperatorWriteNode` — `a.b += 1`

## Variables -- Target (Multi-assign)

- `LocalVariableTargetNode` — `a` in `a, b = ...`
- `InstanceVariableTargetNode` — `@a` in `@a, @b = ...`
- `ClassVariableTargetNode` — `@@a` in `@@a, @@b = ...`
- `GlobalVariableTargetNode` — `$a` in `$a, $b = ...`
- `ConstantTargetNode` — `A` in `A, B = ...`
- `ConstantPathTargetNode` — `Foo::A` in `Foo::A, Foo::B = ...`
- `IndexTargetNode` — `a[0]` in `a[0], a[1] = ...`
- `CallTargetNode` — `a.b` in `a.b, a.c = ...`
- `MultiTargetNode` — `(a, b)` in `(a, b), c = ...` (nested)

## Constants

- `ConstantReadNode` — `Foo`
- `ConstantPathNode` — `Foo::Bar` (parent -> child)
- `ConstantWriteNode` — `FOO = 1`
- `ConstantPathWriteNode` — `Foo::BAR = 1`
- `ShareableConstantNode` — `# shareable_constant_value: literal` magic comment

## Control Flow

- `IfNode` — `if cond ... end` / `x if cond`
- `UnlessNode` — `unless cond ... end` / `x unless cond`
- `ElseNode` — `else ... end` inside if/unless/case
- `CaseNode` — `case x; when ... end`
- `WhenNode` — `when value` inside case
- `CaseMatchNode` — `case x; in pattern ... end` (pattern matching)
- `InNode` — `in pattern` inside case/in
- `WhileNode` — `while cond ... end`
- `UntilNode` — `until cond ... end`
- `ForNode` — `for x in collection ... end`
- `FlipFlopNode` — `if (a)..(b)` (flip-flop range)
- `ReturnNode` — `return x`
- `BreakNode` — `break`
- `NextNode` — `next`
- `RedoNode` — `redo`
- `RetryNode` — `retry` (inside rescue)
- `YieldNode` — `yield x`
- `SuperNode` — `super(args)`
- `ForwardingSuperNode` — `super` (no parens, forwards args)

## Definitions

- `DefNode` — `def foo ... end` / `def self.foo ... end`
- `ClassNode` — `class Foo ... end`
- `ModuleNode` — `module Foo ... end`
- `SingletonClassNode` — `class << obj ... end`
- `AliasMethodNode` — `alias new_name old_name`
- `AliasGlobalVariableNode` — `alias $new $old`
- `UndefNode` — `undef :foo`

## Calls and Blocks

- `CallNode` — `foo.bar(args)` / `foo + bar` / `!x`
- `BlockNode` — `{ |x| ... }` / `do |x| ... end` (child of CallNode)
- `LambdaNode` — `-> (x) { ... }`
- `BlockArgumentNode` — `foo(&block)`
- `ForwardingArgumentsNode` — `foo(...)` (argument forwarding)
- `SplatNode` — `*args` in call
- `AssocNode` — `key => value` / `key: value`
- `AssocSplatNode` — `**hash` in call/hash
- `ArgumentsNode` — container for call arguments
- `ProgramNode` — root node of parse tree
- `StatementsNode` — sequence of statements (body of most nodes)
- `ParenthesesNode` — `(expr)`
- `ImplicitNode` — implicit value (e.g., hash value omission)
- `ImplicitRestNode` — implicit rest `_` in pattern
- `DefinedNode` — `defined?(x)`
- `PreExecutionNode` — `BEGIN { ... }`
- `PostExecutionNode` — `END { ... }`
- `MissingNode` — error recovery placeholder

## Operators

- `AndNode` — `a && b` / `a and b`
- `OrNode` — `a || b` / `a or b`
- `NotNode` — `not x` (keyword not; `!x` is CallNode)
- `MatchWriteNode` — `/(?<name>pattern)/ =~ str` (regex named capture)
- `MatchPredicateNode` — `expr in pattern` (one-line pattern match)
- `MatchRequiredNode` — `expr => pattern` (one-line pattern match, raises)

## Pattern Matching

- `ArrayPatternNode` — `in [a, b, c]`
- `HashPatternNode` — `in { key: value }`
- `FindPatternNode` — `in [*, a, *]`
- `CapturePatternNode` — `in pattern => var`
- `PinnedExpressionNode` — `in ^(expr)`
- `PinnedVariableNode` — `in ^var`
- `AlternationPatternNode` — `in A | B`

## Parameters

- `ParametersNode` — container for def parameters
- `RequiredParameterNode` — `def foo(x)`
- `OptionalParameterNode` — `def foo(x = 1)`
- `RequiredKeywordParameterNode` — `def foo(x:)`
- `OptionalKeywordParameterNode` — `def foo(x: 1)`
- `RestParameterNode` — `def foo(*args)`
- `KeywordRestParameterNode` — `def foo(**opts)`
- `BlockParameterNode` — `def foo(&block)`
- `ForwardingParameterNode` — `def foo(...)`
- `NoKeywordsParameterNode` — `def foo(**nil)`
- `BlockParametersNode` — `{ |x, y| }` block params
- `BlockLocalVariableNode` — `{ |x; local| }` block-local var
- `NumberedParametersNode` — `_1`, `_2` (numbered block params)
- `ItParametersNode` — `it` parameter node (Ruby 3.4)

## Exceptions

- `BeginNode` — `begin ... rescue ... end`
- `RescueNode` — `rescue TypeError => e`
- `RescueModifierNode` — `expr rescue fallback`
- `EnsureNode` — `ensure ... end`
