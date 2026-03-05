# Key Node Accessor Reference (ruby-prism v1.9.0)

Every node struct has these universal methods:

- `location() -> Location<'pr>` -- byte range of entire node
- `as_node() -> Node<'pr>` -- convert back to generic Node enum
- `flags() -> pm_node_flags_t` -- raw flags bitfield

Every `Node` enum has:

- `location() -> Location<'pr>` -- dispatches to variant's location
- `as_*_node() -> Option<SpecificNode<'pr>>` -- typed conversion (one per variant)

## Table of Contents

- [CallNode](#callnode) (most used)
- [DefNode](#defnode)
- [BlockNode](#blocknode)
- [LambdaNode](#lambdanode)
- [IfNode / UnlessNode](#ifnode--unlessnode)
- [CaseNode / CaseMatchNode](#casenode--casematchnode)
- [WhenNode / InNode / ElseNode](#whennode--innode--elsenode)
- [ClassNode / ModuleNode / SingletonClassNode](#classnode--modulenode--singletonclassnode)
- [StatementsNode](#statementsnode)
- [ProgramNode](#programnode)
- [BeginNode / RescueNode / EnsureNode](#beginnode--rescuenode--ensurenode)
- [WhileNode / UntilNode / ForNode](#whilenode--untilnode--fornode)
- [HashNode / AssocNode / KeywordHashNode](#hashnode--assocnode--keywordhashnode)
- [ArrayNode](#arraynode)
- [StringNode / InterpolatedStringNode](#stringnode--interpolatedstringnode)
- [SymbolNode / InterpolatedSymbolNode](#symbolnode--interpolatedsymbolnode)
- [RegularExpressionNode](#regularexpressionnode)
- [AndNode / OrNode](#andnode--ornode)
- [ParenthesesNode](#parenthesesnode)
- [ReturnNode / BreakNode / NextNode](#returnnode--breaknode--nextnode)
- [YieldNode / SuperNode](#yieldnode--supernode)
- [Variable Write Nodes](#variable-write-nodes)
- [Variable Read Nodes](#variable-read-nodes)
- [ConstantPathNode](#constantpathnode)
- [RangeNode](#rangenode)
- [ArgumentsNode / ParametersNode](#argumentsnode--parametersnode)
- [EmbeddedStatementsNode](#embeddedstatementsnode)
- [SplatNode / AssocSplatNode](#splatnode--assocsplatnode)
- [DefinedNode](#definednode)
- [AliasMethodNode](#aliasmethodnode)

---

## CallNode

The most important node -- represents method calls, operators, and attribute access.

```
foo.bar(arg1, arg2) { |x| body }
^^^                                receiver
   ^                               call_operator_loc (.)
    ^^^                            message_loc / name
       ^                           opening_loc (()
            ^^^^^^^^               arguments
                    ^              closing_loc ())
                      ^^^^^^^^^^^^^ block
```

| Method                   | Return Type             | Notes                            |
| ------------------------ | ----------------------- | -------------------------------- |
| `receiver()`             | `Option<Node>`          | `nil` for bare calls like `puts` |
| `call_operator_loc()`    | `Option<Location>`      | `.` or `&.`                      |
| `name()`                 | `ConstantId`            | Method name as bytes             |
| `message_loc()`          | `Option<Location>`      | Position of method name          |
| `opening_loc()`          | `Option<Location>`      | `(` if present                   |
| `arguments()`            | `Option<ArgumentsNode>` | Method arguments                 |
| `closing_loc()`          | `Option<Location>`      | `)` if present                   |
| `block()`                | `Option<Node>`          | BlockNode or BlockArgumentNode   |
| `equal_loc()`            | `Option<Location>`      | `=` for attribute writers        |
| `is_safe_navigation()`   | `bool`                  | `&.` operator flag               |
| `is_variable_call()`     | `bool`                  | Could be local variable          |
| `is_attribute_write()`   | `bool`                  | `foo.bar = val`                  |
| `is_ignore_visibility()` | `bool`                  | Ignores method visibility        |

**Key notes:**

- Operators like `+`, `-`, `[]`, `!` are all CallNode
- `block()` returns BlockNode as child -- NOT the other way around
- `name()` returns `ConstantId` -- use `String::from_utf8_lossy(call.name().as_slice())`

## DefNode

```
def self.foo(params) = body
    ^^^^                       receiver (self)
        ^^^                    name / name_loc
           ^^^^^^^^            parameters
             ^                 lparen_loc
                   ^           rparen_loc
                     ^         equal_loc (endless method)
                       ^^^^    body
```

| Method              | Return Type              | Notes                         |
| ------------------- | ------------------------ | ----------------------------- |
| `name()`            | `ConstantId`             | Method name as bytes          |
| `name_loc()`        | `Location`               | Position of method name       |
| `receiver()`        | `Option<Node>`           | `self` for `def self.foo`     |
| `parameters()`      | `Option<ParametersNode>` | Method parameters             |
| `body()`            | `Option<Node>`           | Usually StatementsNode        |
| `locals()`          | `ConstantList`           | Local variable names in scope |
| `def_keyword_loc()` | `Location`               | Position of `def` keyword     |
| `operator_loc()`    | `Option<Location>`       | `.` in `def self.foo`         |
| `lparen_loc()`      | `Option<Location>`       | `(`                           |
| `rparen_loc()`      | `Option<Location>`       | `)`                           |
| `equal_loc()`       | `Option<Location>`       | `=` for endless methods       |
| `end_keyword_loc()` | `Option<Location>`       | `end` keyword                 |

## BlockNode

Child of CallNode. Has NO `.call()` method.

| Method          | Return Type    | Notes                                                             |
| --------------- | -------------- | ----------------------------------------------------------------- |
| `locals()`      | `ConstantList` | Block-local variable names                                        |
| `parameters()`  | `Option<Node>` | BlockParametersNode or NumberedParametersNode or ItParametersNode |
| `body()`        | `Option<Node>` | Usually StatementsNode                                            |
| `opening_loc()` | `Location`     | `{` or `do`                                                       |
| `closing_loc()` | `Location`     | `}` or `end`                                                      |

## LambdaNode

Separate from BlockNode. Represents `-> { }` / `-> do end`.

| Method           | Return Type    | Notes                       |
| ---------------- | -------------- | --------------------------- |
| `locals()`       | `ConstantList` | Lambda-local variable names |
| `operator_loc()` | `Location`     | `->` position               |
| `opening_loc()`  | `Location`     | `{` or `do`                 |
| `closing_loc()`  | `Location`     | `}` or `end`                |
| `parameters()`   | `Option<Node>` | BlockParametersNode         |
| `body()`         | `Option<Node>` | Usually StatementsNode      |

## IfNode / UnlessNode

| Method                               | Return Type              | Notes                          |
| ------------------------------------ | ------------------------ | ------------------------------ |
| `if_keyword_loc()` / `keyword_loc()` | `Option<Location>`       | `if`/`unless`/`elsif` keyword  |
| `predicate()`                        | `Node`                   | Condition expression           |
| `then_keyword_loc()`                 | `Option<Location>`       | `then` keyword                 |
| `statements()`                       | `Option<StatementsNode>` | Then-branch body               |
| `subsequent()`                       | `Option<Node>`           | ElseNode or IfNode (for elsif) |
| `end_keyword_loc()`                  | `Option<Location>`       | `end` keyword                  |

**Key note:** `subsequent()` returns `IfNode` for `elsif` chains, `ElseNode` for `else`.

## CaseNode / CaseMatchNode

| Method               | Return Type        | Notes                                                 |
| -------------------- | ------------------ | ----------------------------------------------------- |
| `predicate()`        | `Option<Node>`     | The `case expr` expression                            |
| `conditions()`       | `NodeList`         | List of WhenNode (CaseNode) or InNode (CaseMatchNode) |
| `else_clause()`      | `Option<ElseNode>` | The `else` branch                                     |
| `case_keyword_loc()` | `Location`         | `case` keyword                                        |
| `end_keyword_loc()`  | `Location`         | `end` keyword                                         |

## WhenNode / InNode / ElseNode

**WhenNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `conditions()` | `NodeList` | When values (`when 1, 2, 3`) |
| `statements()` | `Option<StatementsNode>` | When body |
| `keyword_loc()` | `Location` | `when` keyword |
| `then_keyword_loc()` | `Option<Location>` | `then` keyword |

**InNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `pattern()` | `Node` | Pattern to match |
| `statements()` | `Option<StatementsNode>` | In body |
| `in_loc()` | `Location` | `in` keyword |
| `then_loc()` | `Option<Location>` | `then` keyword |

**ElseNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `statements()` | `Option<StatementsNode>` | Else body |
| `else_keyword_loc()` | `Location` | `else` keyword |

## ClassNode / ModuleNode / SingletonClassNode

**ClassNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `locals()` | `ConstantList` | Local variables |
| `class_keyword_loc()` | `Location` | `class` keyword |
| `constant_path()` | `Node` | Class name (ConstantReadNode or ConstantPathNode) |
| `inheritance_operator_loc()` | `Option<Location>` | `<` |
| `superclass()` | `Option<Node>` | Parent class |
| `body()` | `Option<Node>` | Usually StatementsNode |
| `end_keyword_loc()` | `Location` | `end` keyword |
| `name()` | `ConstantId` | Simple class name |

**ModuleNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `locals()` | `ConstantList` | Local variables |
| `module_keyword_loc()` | `Location` | `module` keyword |
| `constant_path()` | `Node` | Module name |
| `body()` | `Option<Node>` | Usually StatementsNode |
| `end_keyword_loc()` | `Location` | `end` keyword |
| `name()` | `ConstantId` | Simple module name |

**SingletonClassNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `locals()` | `ConstantList` | Local variables |
| `expression()` | `Node` | The object (`class << obj`) |
| `body()` | `Option<Node>` | Usually StatementsNode |

## StatementsNode

| Method   | Return Type | Notes                   |
| -------- | ----------- | ----------------------- |
| `body()` | `NodeList`  | List of statement nodes |

Most bodies in the AST are wrapped in StatementsNode. Always check.

## ProgramNode

| Method         | Return Type      | Notes                     |
| -------------- | ---------------- | ------------------------- |
| `locals()`     | `ConstantList`   | Top-level local variables |
| `statements()` | `StatementsNode` | Program body (NOT Option) |

## BeginNode / RescueNode / EnsureNode

**BeginNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `begin_keyword_loc()` | `Option<Location>` | `begin` keyword |
| `statements()` | `Option<StatementsNode>` | Main body |
| `rescue_clause()` | `Option<RescueNode>` | First rescue |
| `else_clause()` | `Option<ElseNode>` | Else branch |
| `ensure_clause()` | `Option<EnsureNode>` | Ensure block |
| `end_keyword_loc()` | `Option<Location>` | `end` keyword |

**RescueNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `keyword_loc()` | `Location` | `rescue` keyword |
| `exceptions()` | `NodeList` | Exception classes to catch |
| `operator_loc()` | `Option<Location>` | `=>` operator |
| `reference()` | `Option<Node>` | Variable to assign exception |
| `statements()` | `Option<StatementsNode>` | Rescue body |
| `subsequent()` | `Option<RescueNode>` | Next rescue clause (chain) |

**EnsureNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `ensure_keyword_loc()` | `Location` | `ensure` keyword |
| `statements()` | `Option<StatementsNode>` | Ensure body |
| `end_keyword_loc()` | `Location` | `end` keyword |

## WhileNode / UntilNode / ForNode

**WhileNode / UntilNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `keyword_loc()` | `Location` | `while`/`until` keyword |
| `closing_loc()` | `Option<Location>` | `end` keyword |
| `predicate()` | `Node` | Loop condition |
| `statements()` | `Option<StatementsNode>` | Loop body |

**ForNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `index()` | `Node` | Loop variable |
| `collection()` | `Node` | Iterable |
| `statements()` | `Option<StatementsNode>` | Loop body |
| `for_keyword_loc()` | `Location` | `for` keyword |
| `in_keyword_loc()` | `Location` | `in` keyword |
| `do_keyword_loc()` | `Option<Location>` | `do` keyword |
| `end_keyword_loc()` | `Location` | `end` keyword |

## HashNode / AssocNode / KeywordHashNode

**HashNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `opening_loc()` | `Location` | `{` |
| `elements()` | `NodeList` | AssocNode and AssocSplatNode items |
| `closing_loc()` | `Location` | `}` |

**AssocNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `key()` | `Node` | Hash key (SymbolNode, StringNode, etc.) |
| `value()` | `Node` | Hash value |
| `operator_loc()` | `Option<Location>` | `=>` or `:` (None for shorthand `x:`) |

**KeywordHashNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `elements()` | `NodeList` | Same as HashNode but for implicit hashes in method args |

## ArrayNode

| Method          | Return Type        | Notes             |
| --------------- | ------------------ | ----------------- |
| `opening_loc()` | `Option<Location>` | `[` or `%w(` etc. |
| `elements()`    | `NodeList`         | Array elements    |
| `closing_loc()` | `Option<Location>` | `]` or `)` etc.   |

## StringNode / InterpolatedStringNode

**StringNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `opening_loc()` | `Option<Location>` | Quote char (None for `__FILE__` etc.) |
| `content_loc()` | `Location` | String content position |
| `closing_loc()` | `Option<Location>` | Closing quote |
| `unescaped()` | `&[u8]` | Unescaped string content |
| `is_frozen()` | `bool` | Frozen string flag |
| `is_forced_utf8_encoding()` | `bool` | UTF-8 encoding flag |
| `is_forced_binary_encoding()` | `bool` | Binary encoding flag |

**InterpolatedStringNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `opening_loc()` | `Option<Location>` | Opening quote |
| `parts()` | `NodeList` | Mix of StringNode and EmbeddedStatementsNode |
| `closing_loc()` | `Option<Location>` | Closing quote |
| `is_frozen()` | `bool` | Frozen string flag |
| `is_mutable()` | `bool` | Mutable string flag |

## SymbolNode / InterpolatedSymbolNode

**SymbolNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `opening_loc()` | `Option<Location>` | `:` or `%s(` |
| `value_loc()` | `Option<Location>` | Symbol value position |
| `closing_loc()` | `Option<Location>` | Closing delimiter |
| `unescaped()` | `&[u8]` | Symbol value |
| `is_forced_utf8_encoding()` | `bool` | UTF-8 flag |
| `is_forced_binary_encoding()` | `bool` | Binary flag |
| `is_forced_us_ascii_encoding()` | `bool` | US-ASCII flag |

**InterpolatedSymbolNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `opening_loc()` | `Option<Location>` | Opening delimiter |
| `parts()` | `NodeList` | Mix of StringNode and EmbeddedStatementsNode |
| `closing_loc()` | `Option<Location>` | Closing delimiter |

## RegularExpressionNode

| Method             | Return Type | Notes              |
| ------------------ | ----------- | ------------------ |
| `opening_loc()`    | `Location`  | `/` or `%r(`       |
| `content_loc()`    | `Location`  | Pattern content    |
| `closing_loc()`    | `Location`  | `/` or `)` + flags |
| `unescaped()`      | `&[u8]`     | Unescaped pattern  |
| `is_ignore_case()` | `bool`      | `i` flag           |
| `is_extended()`    | `bool`      | `x` flag           |
| `is_multi_line()`  | `bool`      | `m` flag           |
| `is_once()`        | `bool`      | `o` flag           |

## AndNode / OrNode

| Method           | Return Type | Notes                      |
| ---------------- | ----------- | -------------------------- |
| `left()`         | `Node`      | Left operand (NOT Option)  |
| `right()`        | `Node`      | Right operand (NOT Option) |
| `operator_loc()` | `Location`  | `&&`/`and` or `\|\|`/`or`  |

## ParenthesesNode

| Method          | Return Type    | Notes                  |
| --------------- | -------------- | ---------------------- |
| `body()`        | `Option<Node>` | Usually StatementsNode |
| `opening_loc()` | `Location`     | `(`                    |
| `closing_loc()` | `Location`     | `)`                    |

## ReturnNode / BreakNode / NextNode

| Method          | Return Type             | Notes                           |
| --------------- | ----------------------- | ------------------------------- |
| `keyword_loc()` | `Location`              | `return`/`break`/`next` keyword |
| `arguments()`   | `Option<ArgumentsNode>` | Return value(s)                 |

## YieldNode / SuperNode

**YieldNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `keyword_loc()` | `Location` | `yield` keyword |
| `lparen_loc()` | `Option<Location>` | `(` |
| `arguments()` | `Option<ArgumentsNode>` | Yield arguments |
| `rparen_loc()` | `Option<Location>` | `)` |

**SuperNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `keyword_loc()` | `Location` | `super` keyword |
| `lparen_loc()` | `Option<Location>` | `(` |
| `arguments()` | `Option<ArgumentsNode>` | Super arguments |
| `rparen_loc()` | `Option<Location>` | `)` |
| `block()` | `Option<Node>` | Block passed to super |

## Variable Write Nodes

All `*WriteNode` types (LocalVariableWriteNode, InstanceVariableWriteNode, etc.):

| Method           | Return Type  | Notes                  |
| ---------------- | ------------ | ---------------------- |
| `name()`         | `ConstantId` | Variable name          |
| `name_loc()`     | `Location`   | Variable name position |
| `value()`        | `Node`       | Assigned value         |
| `operator_loc()` | `Location`   | `=` operator           |

## Variable Read Nodes

All `*ReadNode` types (LocalVariableReadNode, InstanceVariableReadNode, etc.):

| Method   | Return Type  | Notes         |
| -------- | ------------ | ------------- |
| `name()` | `ConstantId` | Variable name |

`ConstantReadNode` also has only `name()`.

## ConstantPathNode

| Method            | Return Type          | Notes                                |
| ----------------- | -------------------- | ------------------------------------ |
| `parent()`        | `Option<Node>`       | Left side of `::` (None for `::Foo`) |
| `name()`          | `Option<ConstantId>` | Right side name                      |
| `delimiter_loc()` | `Location`           | `::` position                        |
| `name_loc()`      | `Location`           | Name position                        |

## RangeNode

| Method             | Return Type    | Notes                           |
| ------------------ | -------------- | ------------------------------- |
| `left()`           | `Option<Node>` | Start of range (None for `..5`) |
| `right()`          | `Option<Node>` | End of range (None for `5..`)   |
| `operator_loc()`   | `Location`     | `..` or `...`                   |
| `is_exclude_end()` | `bool`         | `...` (exclusive) flag          |

## ArgumentsNode / ParametersNode

**ArgumentsNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `arguments()` | `NodeList` | List of argument nodes |
| `is_contains_forwarding()` | `bool` | Contains `...` forwarding |
| `is_contains_keywords()` | `bool` | Contains keyword args |
| `is_contains_keyword_splat()` | `bool` | Contains `**` splat |
| `is_contains_splat()` | `bool` | Contains `*` splat |

**ParametersNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `requireds()` | `NodeList` | Required positional params |
| `optionals()` | `NodeList` | Optional positional params |
| `rest()` | `Option<Node>` | Rest parameter (`*args`) |
| `posts()` | `NodeList` | Post-rest required params |
| `keywords()` | `NodeList` | Keyword params |
| `keyword_rest()` | `Option<Node>` | `**kwargs` or `**nil` |
| `block()` | `Option<BlockParameterNode>` | `&block` param |

## EmbeddedStatementsNode

Inside string interpolation `#{...}`:

| Method          | Return Type              | Notes                   |
| --------------- | ------------------------ | ----------------------- |
| `opening_loc()` | `Location`               | `#{`                    |
| `statements()`  | `Option<StatementsNode>` | Interpolated expression |
| `closing_loc()` | `Location`               | `}`                     |

## SplatNode / AssocSplatNode

**SplatNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `operator_loc()` | `Location` | `*` position |
| `expression()` | `Option<Node>` | Splatted expression |

**AssocSplatNode:**
| Method | Return Type | Notes |
|--------|-------------|-------|
| `operator_loc()` | `Location` | `**` position |
| `value()` | `Option<Node>` | Double-splatted expression |

## DefinedNode

| Method          | Return Type        | Notes                    |
| --------------- | ------------------ | ------------------------ |
| `keyword_loc()` | `Location`         | `defined?` keyword       |
| `lparen_loc()`  | `Option<Location>` | `(`                      |
| `value()`       | `Node`             | Expression being checked |
| `rparen_loc()`  | `Option<Location>` | `)`                      |

## AliasMethodNode

| Method          | Return Type | Notes                |
| --------------- | ----------- | -------------------- |
| `keyword_loc()` | `Location`  | `alias` keyword      |
| `new_name()`    | `Node`      | New method name      |
| `old_name()`    | `Node`      | Existing method name |
