---
name: ruby-prism-api
description: "Comprehensive API reference for the ruby-prism Rust crate (v1.9.0) used in ruby-fast-cop. MUST consult when implementing new RuboCop cops, debugging Prism-related compilation errors (E0282, E0106, E0599), or working with AST node traversal. Covers node ownership model, BlockNode/CallNode relationship, type inference gotchas, Visit trait patterns, and all 148 node types with their accessors."
---

# ruby-prism Rust Crate API Reference (v1.9.0)

## Critical Rules (Read First)

### 1. Node Ownership -- No Clone, No Copy

`Node<'pr>` does NOT implement `Clone` or `Copy`. All 148 variants are struct wrappers around raw pointers with `PhantomData` lifetime markers.

```rust
// WRONG: Node doesn't implement Clone
let saved = node.clone(); // Only clones the reference, NOT the node

// WRONG: Cannot return &Node from a local Vec
fn get_first<'a>(stmts: &StatementsNode<'a>) -> &Node<'a> {
    let items: Vec<_> = stmts.body().iter().collect(); // Vec is local
    &items[0] // ERROR: returns reference to local data
}

// CORRECT: Return Option<SpecificNodeType> (owned)
fn extract_call<'a>(node: &Node<'a>) -> Option<CallNode<'a>> {
    node.as_call_node() // Returns owned CallNode, not reference
}

// CORRECT: Use predicate closures instead of returning Node
fn base_receiver_matches(node: &Node, pred: impl Fn(&Node) -> bool) -> bool {
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            match call.receiver() {
                Some(recv) => base_receiver_matches(&recv, pred),
                None => pred(node),
            }
        }
        _ => pred(node),
    }
}

// CORRECT: Use recursion instead of loops needing reassignment
fn walk_chain(node: &Node) -> usize {
    if let Some(call) = node.as_call_node() {
        if let Some(recv) = call.receiver() {
            return 1 + walk_chain(&recv); // Recurse, don't loop
        }
    }
    1
}
```

### 2. BlockNode/CallNode Relationship

In Prism, `BlockNode` is a **child** of `CallNode`. This is opposite to other Ruby parsers.

```rust
// WRONG: BlockNode has NO .call() method
let call = block_node.call(); // ERROR: E0599 no method `call`

// CORRECT: Access block from CallNode
let block: Option<Node> = call_node.block();
if call_node.block().is_some() { /* has a block */ }

// BlockNode only has: locals(), parameters(), body(), opening_loc(), closing_loc()
// LambdaNode is separate from BlockNode
```

### 3. Type Inference Gotchas (E0282)

Prism's generic return types cause Rust type inference failures with closures:

```rust
// WRONG: E0282 type annotations needed
let has_args = call.arguments().map_or(false, |args| args.arguments().iter().count() > 0);
let msg = call.message_loc().is_some_and(|loc| loc.as_slice() == b"foo");

// CORRECT: Use if-let pattern
let has_args = if let Some(args) = call.arguments() {
    let arg_list: Vec<_> = args.arguments().iter().collect();
    !arg_list.is_empty()
} else {
    false
};

if let Some(msg_loc) = call.message_loc() {
    let name = msg_loc.as_slice();
    // ...
}
```

### 4. NodeList Iteration

`NodeList.iter()` yields owned `Node<'pr>` values. Collect to Vec for multi-pass or indexing:

```rust
let items: Vec<_> = stmts.body().iter().collect();
// Now: items[0], items.len(), items.windows(2), etc.

// Single-pass is fine without collecting:
for node in stmts.body().iter() { /* ... */ }

// NodeList also has: len(), is_empty(), first(), last()
```

### 5. Names Are Bytes

Node names return `ConstantId`, not `&str`:

```rust
let method_name = String::from_utf8_lossy(call.name().as_slice());
let method_str: &str = method_name.as_ref();
if method_str == "puts" { /* ... */ }
```

### 6. Location API

Locations provide byte offsets only -- no line/column:

```rust
let loc = node.location();
let start: usize = loc.start_offset();
let end: usize = loc.end_offset();
let bytes: &[u8] = loc.as_slice();
let source_text: &str = &ctx.source[start..end];

// Specific locations on nodes:
call.message_loc()        // Option<Location> -- method name position
call.call_operator_loc()  // Option<Location> -- `.` or `&.` position
call.opening_loc()        // Option<Location> -- `(` position
call.closing_loc()        // Option<Location> -- `)` position
def_node.name_loc()       // Location -- method name in definition

// Join two locations:
let combined: Option<Location> = loc1.join(&loc2);
```

## Visit Trait Pattern

```rust
use ruby_prism::Visit;

struct MyVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for MyVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Custom logic here
        // MUST call default to continue traversal:
        ruby_prism::visit_call_node(self, node);
    }

    // Stop traversal at certain nodes (don't call default):
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {
        // Not calling ruby_prism::visit_def_node stops recursion
    }

    // Depth tracking:
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.depth -= 1;
    }
}

// Run the visitor:
let mut visitor = MyVisitor { ctx: &ctx, offenses: vec![] };
visitor.visit(&parse_result.node());
```

## Node Matching Patterns

```rust
// Pattern 1: Match variant then convert (most common)
match node {
    Node::CallNode { .. } => {
        let call = node.as_call_node().unwrap();
        // use call.receiver(), call.name(), etc.
    }
    Node::IfNode { .. } => { /* ... */ }
    _ => {}
}

// Pattern 2: if-let for single type check
if let Some(call) = node.as_call_node() {
    // use call directly
}

// Pattern 3: matches! for boolean check
if matches!(node, Node::TrueNode { .. } | Node::FalseNode { .. }) {
    // is a boolean literal
}
```

## Detailed Node References

- **All 148 node types by category**: See [references/node-types.md](references/node-types.md)
- **Key node accessor methods and return types**: See [references/node-accessors.md](references/node-accessors.md)

## Anti-Patterns Checklist

Before submitting code, verify:

- No `block_node.call()` -- use `call_node.block()` instead
- No `node.clone()` expecting deep copy -- restructure to avoid
- No `&Node` returned from locally-collected Vec
- No `.map_or()` / `.is_some_and()` on Prism Optional returns with closures
- No loops with `node = next_node` -- use recursion
- All NodeList access collected to Vec before indexing
- Names converted via `String::from_utf8_lossy(x.name().as_slice())`
