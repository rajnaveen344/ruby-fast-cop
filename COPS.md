# All Cops State (606 total)

Full list of all RuboCop cops tracked by ruby-fast-cop, organized by department and default status.
606 of 606 implemented (396 enabled-by-default + 156 pending-by-default + 54 disabled-by-default). See [README.md](README.md) for the implementation roadmap.

**Pending-default progress: 156 / 156 (100%)**. **Disabled-default progress: 54 / 54 (100%)**.

**Autocorrect progress: 9,399 / 11,217 (84%)** — 1,818 expected corrections across ~115 cops still unwired. See [`.correction_worklist.txt`](.correction_worklist.txt) for per-cop counts.

## Summary

Cop-count cells show `implemented / total`. Autocorrect column shows `wired / expected (%)`, where "expected" = TOML fixture cases with a `corrected` block and "wired" = cop currently emits a `Correction` that produces matching corrected source.

| Department |     Enabled |     Pending |  Disabled |             Tests |              Autocorrect |
| ---------- | ----------: | ----------: | --------: | ----------------: | -----------------------: |
| Style      |     175/175 |       91/91 |     32/32 |   14,566 / 14,566 |      5,769 / 7,318 (79%) |
| Lint       |     100/100 |       50/50 |       4/4 |     5,961 / 5,961 |      1,766 / 1,908 (93%) |
| Layout     |       81/81 |         5/5 |     14/14 |     4,646 / 4,646 |      1,775 / 1,851 (96%) |
| Metrics    |         9/9 |         1/1 |       0/0 |         272 / 272 |         n/a (0 expected) |
| Naming     |       16/16 |         2/2 |       1/1 |     2,216 / 2,216 |            67 / 86 (78%) |
| Gemspec    |         4/4 |         5/5 |       1/1 |         193 / 193 |           24 / 24 (100%) |
| Bundler    |         5/5 |         0/0 |       2/2 |         101 / 101 |             8 / 12 (66%) |
| Security   |         5/5 |         2/2 |       0/0 |         102 / 102 |            12 / 17 (70%) |
| Migration  |         1/1 |         0/0 |       0/0 |             8 / 8 |             1 / 1 (100%) |
| **Total**  | **396/396** | **156/156** | **54/54** | **28,065/28,065** | **9,399 / 11,217 (84%)** |

- **Enabled**: Runs by default on every codebase (highest priority to implement)
- **Pending**: Runs only with `NewCops: enable` in config
- **Disabled**: Runs only when explicitly enabled in config

## Style (198/298 implemented, 14,567 tests)

### Enabled by Default (175 cops, 9,202 tests)

| Cop                                    | Tests | Status      |
| -------------------------------------- | ----: | ----------- |
| Style/AccessModifierDeclarations       |   377 | Implemented |
| Style/AccessorGrouping                 |    26 | Implemented |
| Style/Alias                            |    26 | Implemented |
| Style/AndOr                            |    76 | Implemented |
| Style/ArrayIntersect                   |    81 | Implemented |
| Style/ArrayIntersectWithSingleElement  |     3 | Implemented |
| Style/ArrayJoin                        |     5 | Implemented |
| Style/Attr                             |    11 | Implemented |
| Style/BarePercentLiterals              |    36 | Implemented |
| Style/BeginBlock                       |     1 | Implemented |
| Style/BisectedAttrAccessor             |    14 | Implemented |
| Style/BlockComments                    |     5 | Implemented |
| Style/BlockDelimiters                  |   173 | Implemented |
| Style/CaseEquality                     |    25 | Implemented |
| Style/CaseLikeIf                       |    38 | Implemented |
| Style/CharacterLiteral                 |     5 | Implemented |
| Style/ClassAndModuleChildren           |    40 | Implemented |
| Style/ClassCheck                       |     4 | Implemented |
| Style/ClassEqualityComparison          |    22 | Implemented |
| Style/ClassMethods                     |     5 | Implemented |
| Style/ClassVars                        |     5 | Implemented |
| Style/ColonMethodCall                  |    10 | Implemented |
| Style/ColonMethodDefinition            |     3 | Implemented |
| Style/CombinableLoops                  |    20 | Implemented |
| Style/CommandLiteral                   |    35 | Implemented |
| Style/CommentAnnotation                |    31 | Implemented |
| Style/CommentedKeyword                 |    47 | Implemented |
| Style/ConditionalAssignment            |  1199 | Implemented |
| Style/DefWithParentheses               |     9 | Implemented |
| Style/Dir                              |     4 | Implemented |
| Style/Documentation                    |    55 | Implemented |
| Style/DoubleCopDisableDirective        |     3 | Implemented |
| Style/DoubleNegation                   |    47 | Implemented |
| Style/EachForSimpleLoop                |    20 | Implemented |
| Style/EachWithObject                   |    16 | Implemented |
| Style/EmptyBlockParameter              |     9 | Implemented |
| Style/EmptyCaseCondition               |    29 | Implemented |
| Style/EmptyElse                        |   124 | Implemented |
| Style/EmptyLambdaParameter             |     3 | Implemented |
| Style/EmptyLiteral                     |    49 | Implemented |
| Style/EmptyMethod                      |    32 | Implemented |
| Style/Encoding                         |    13 | Implemented |
| Style/EndBlock                         |     2 | Implemented |
| Style/EvalWithLocation                 |    27 | Implemented |
| Style/EvenOdd                          |    18 | Implemented |
| Style/ExpandPathArguments              |    16 | Implemented |
| Style/ExplicitBlockArgument            |    21 | Implemented |
| Style/ExponentialNotation              |    27 | Implemented |
| Style/FloatDivision                    |    31 | Implemented |
| Style/For                              |    32 | Implemented |
| Style/FormatString                     |    46 | Implemented |
| Style/FormatStringToken                |   366 | Implemented |
| Style/FrozenStringLiteralComment       |   107 | Implemented |
| Style/GlobalStdStream                  |     6 | Implemented |
| Style/GlobalVars                       |    74 | Implemented |
| Style/GuardClause                      |    91 | Implemented |
| Style/HashAsLastArrayItem              |    19 | Implemented |
| Style/HashEachMethods                  |    62 | Implemented |
| Style/HashLikeCase                     |     8 | Implemented |
| Style/HashSyntax                       |   189 | Implemented |
| Style/HashTransformKeys                |    40 | Implemented |
| Style/HashTransformValues              |    40 | Implemented |
| Style/IdenticalConditionalBranches     |    48 | Implemented |
| Style/IfInsideElse                     |    21 | Implemented |
| Style/IfUnlessModifier                 |   126 | Implemented |
| Style/IfUnlessModifierOfIfUnless       |     7 | Implemented |
| Style/IfWithSemicolon                  |    28 | Implemented |
| Style/InfiniteLoop                     |    28 | Implemented |
| Style/InverseMethods                   |   110 | Implemented |
| Style/KeywordParametersOrder           |    10 | Implemented |
| Style/Lambda                           |    38 | Implemented |
| Style/LambdaCall                       |    19 | Implemented |
| Style/LineEndConcatenation             |    19 | Implemented |
| Style/MethodCallWithoutArgsParentheses |    34 | Implemented |
| Style/MethodDefParentheses             |    49 | Implemented |
| Style/MinMax                           |    12 | Implemented |
| Style/MissingRespondToMissing          |     8 | Implemented |
| Style/MixinGrouping                    |    18 | Implemented |
| Style/MixinUsage                       |    18 | Implemented |
| Style/ModuleFunction                   |    11 | Implemented |
| Style/MultilineBlockChain              |    11 | Implemented |
| Style/MultilineIfModifier              |    10 | Implemented |
| Style/MultilineIfThen                  |    11 | Implemented |
| Style/MultilineMemoization             |    17 | Implemented |
| Style/MultilineTernaryOperator         |    17 | Implemented |
| Style/MultilineWhenThen                |    13 | Implemented |
| Style/MultipleComparison               |    34 | Implemented |
| Style/MutableConstant                  |   354 | Implemented |
| Style/NegatedIf                        |    15 | Implemented |
| Style/NegatedUnless                    |    14 | Implemented |
| Style/NegatedWhile                     |    10 | Implemented |
| Style/NestedModifier                   |    13 | Implemented |
| Style/NestedParenthesizedCalls         |    12 | Implemented |
| Style/NestedTernaryOperator            |     7 | Implemented |
| Style/Next                             |    72 | Implemented |
| Style/NilComparison                    |     8 | Implemented |
| Style/NonNilCheck                      |    21 | Implemented |
| Style/Not                              |     9 | Implemented |
| Style/NumericLiteralPrefix             |    10 | Implemented |
| Style/NumericLiterals                  |    28 | Implemented |
| Style/NumericPredicate                 |    43 | Implemented |
| Style/OneLineConditional               |   108 | Implemented |
| Style/OptionalArguments                |    12 | Implemented |
| Style/OptionalBooleanParameter         |     8 | Implemented |
| Style/OrAssignment                     |    25 | Implemented |
| Style/ParallelAssignment               |    86 | Implemented |
| Style/ParenthesesAroundCondition       |    30 | Implemented |
| Style/PercentLiteralDelimiters         |    65 | Implemented |
| Style/PercentQLiterals                 |    21 | Implemented |
| Style/PerlBackrefs                     |    14 | Implemented |
| Style/PreferredHashMethods             |     9 | Implemented |
| Style/Proc                             |     6 | Implemented |
| Style/RaiseArgs                        |    35 | Implemented |
| Style/RandomWithOffset                 |    29 | Implemented |
| Style/RedundantAssignment              |    11 | Implemented |
| Style/RedundantBegin                   |    63 | Implemented |
| Style/RedundantCapitalW                |    13 | Implemented |
| Style/RedundantCondition               |   102 | Implemented |
| Style/RedundantConditional             |    11 | Implemented |
| Style/RedundantException               |    30 | Implemented |
| Style/RedundantFetchBlock              |    15 | Implemented |
| Style/RedundantFileExtensionInRequire  |     4 | Implemented |
| Style/RedundantFreeze                  |    62 | Implemented |
| Style/RedundantInterpolation           |    29 | Implemented |
| Style/RedundantParentheses             |   331 | Implemented |
| Style/RedundantPercentQ                |    25 | Implemented |
| Style/RedundantRegexpCharacterClass    |    47 | Implemented |
| Style/RedundantRegexpEscape            |   217 | Implemented |
| Style/RedundantReturn                  |    39 | Implemented |
| Style/RedundantSelf                    |    62 | Implemented |
| Style/RedundantSelfAssignment          |    14 | Implemented |
| Style/RedundantSort                    |    50 | Implemented |
| Style/RedundantSortBy                  |     8 | Implemented |
| Style/RegexpLiteral                    |    57 | Implemented |
| Style/RescueModifier                   |    21 | Implemented |
| Style/RescueStandardError              |    37 | Implemented |
| Style/SafeNavigation                   |   786 | Implemented |
| Style/Sample                           |    82 | Implemented |
| Style/SelfAssignment                   |   105 | Implemented |
| Style/Semicolon                        |    33 | Implemented |
| Style/SignalException                  |    27 | Implemented |
| Style/SingleArgumentDig                |    15 | Implemented |
| Style/SingleLineMethods                |    16 | Implemented |
| Style/SlicingWithRange                 |    28 | Implemented |
| Style/SoleNestedConditional            |    73 | Implemented |
| Style/SpecialGlobalVars                |    31 | Implemented |
| Style/StabbyLambdaParentheses          |     9 | Implemented |
| Style/StderrPuts                       |     5 | Implemented |
| Style/StringConcatenation              |    30 | Implemented |
| Style/StringLiterals                   |    58 | Implemented |
| Style/StringLiteralsInInterpolation    |    13 | Implemented |
| Style/Strip                            |     6 | Implemented |
| Style/StructInheritance                |    12 | Implemented |
| Style/SymbolArray                      |    33 | Implemented |
| Style/SymbolLiteral                    |     4 | Implemented |
| Style/SymbolProc                       |    83 | Implemented |
| Style/TernaryParentheses               |    98 | Implemented |
| Style/TrailingBodyOnClass              |     7 | Implemented |
| Style/TrailingBodyOnMethodDefinition   |    12 | Implemented |
| Style/TrailingBodyOnModule             |     7 | Implemented |
| Style/TrailingCommaInArguments         |   178 | Implemented |
| Style/TrailingCommaInArrayLiteral      |    48 | Implemented |
| Style/TrailingCommaInHashLiteral       |    41 | Implemented |
| Style/TrailingMethodEndStatement       |    10 | Implemented |
| Style/TrailingUnderscoreVariable       |    58 | Implemented |
| Style/TrivialAccessors                 |    38 | Implemented |
| Style/UnlessElse                       |     5 | Implemented |
| Style/UnpackFirst                      |    11 | Implemented |
| Style/VariableInterpolation            |     9 | Implemented |
| Style/WhenThen                         |     4 | Implemented |
| Style/WhileUntilDo                     |     6 | Implemented |
| Style/WhileUntilModifier               |    34 | Implemented |
| Style/WordArray                        |    59 | Implemented |
| Style/YodaCondition                    |    73 | Implemented |
| Style/ZeroLengthPredicate              |    68 | Implemented |

### Pending by Default (91 cops, 4,624 tests)

| Cop                                        | Tests | Status      |
| ------------------------------------------ | ----: | ----------- |
| Style/AmbiguousEndlessMethodDefinition     |    31 | Implemented |
| Style/ArgumentsForwarding                  |   187 | Implemented |
| Style/BitwisePredicate                     |    18 | Implemented |
| Style/CollectionCompact                    |    30 | Implemented |
| Style/CollectionQuerying                   |    20 | Implemented |
| Style/CombinableDefined                    |    39 | Implemented |
| Style/ComparableBetween                    |    15 | Implemented |
| Style/ComparableClamp                      |    23 | Implemented |
| Style/ConcatArrayLiterals                  |    14 | Implemented |
| Style/DataInheritance                      |    24 | Implemented |
| Style/DigChain                             |    23 | Implemented |
| Style/DirEmpty                             |    16 | Implemented |
| Style/DocumentDynamicEvalDefinition        |    18 | Implemented |
| Style/EmptyClassDefinition                 |    44 | Implemented |
| Style/EmptyHeredoc                         |     7 | Implemented |
| Style/EmptyStringInsideInterpolation       |    24 | Implemented |
| Style/EndlessMethod                        |    63 | Implemented |
| Style/EnvHome                              |     7 | Implemented |
| Style/ExactRegexpMatch                     |    14 | Implemented |
| Style/FetchEnvVar                          |    43 | Implemented |
| Style/FileEmpty                            |    27 | Implemented |
| Style/FileNull                             |    13 | Implemented |
| Style/FileOpen                             |    14 | Implemented |
| Style/FileRead                             |    30 | Implemented |
| Style/FileTouch                            |     4 | Implemented |
| Style/FileWrite                            |    32 | Implemented |
| Style/HashConversion                       |    22 | Implemented |
| Style/HashExcept                           |   114 | Implemented |
| Style/HashFetchChain                       |    35 | Implemented |
| Style/HashSlice                            |   116 | Implemented |
| Style/IfWithBooleanLiteralBranches         |    94 | Implemented |
| Style/InPatternThen                        |     7 | Implemented |
| Style/ItAssignment                         |    23 | Implemented |
| Style/ItBlockParameter                     |    34 | Implemented |
| Style/KeywordArgumentsMerging              |     9 | Implemented |
| Style/MagicCommentFormat                   |    25 | Implemented |
| Style/MapCompactWithConditionalBlock       |    33 | Implemented |
| Style/MapIntoArray                         |    64 | Implemented |
| Style/MapJoin                              |    24 | Implemented |
| Style/MapToHash                            |    38 | Implemented |
| Style/MapToSet                             |    32 | Implemented |
| Style/MinMaxComparison                     |    17 | Implemented |
| Style/ModuleMemberExistenceCheck           |   101 | Implemented |
| Style/MultilineInPatternThen               |    13 | Implemented |
| Style/NegatedIfElseCondition               |    32 | Implemented |
| Style/NegativeArrayIndex                   |   423 | Implemented |
| Style/NestedFileDirname                    |     5 | Implemented |
| Style/NilLambda                            |    31 | Implemented |
| Style/NumberedParameters                   |     4 | Implemented |
| Style/NumberedParametersLimit              |    12 | Implemented |
| Style/ObjectThen                           |    23 | Implemented |
| Style/OneClassPerFile                      |    21 | Implemented |
| Style/OpenStructUse                        |    12 | Implemented |
| Style/OperatorMethodCall                   |   202 | Implemented |
| Style/PartitionInsteadOfDoubleSelect       |    37 | Implemented |
| Style/PredicateWithKind                    |    64 | Implemented |
| Style/QuotedSymbols                        |    97 | Implemented |
| Style/ReduceToHash                         |    20 | Implemented |
| Style/RedundantArgument                    |    15 | Implemented |
| Style/RedundantArrayConstructor            |    13 | Implemented |
| Style/RedundantArrayFlatten                |    10 | Implemented |
| Style/RedundantConstantBase                |     8 | Implemented |
| Style/RedundantCurrentDirectoryInPath      |    12 | Implemented |
| Style/RedundantDoubleSplatHashBraces       |    29 | Implemented |
| Style/RedundantEach                        |    33 | Implemented |
| Style/RedundantFilterChain                 |    39 | Implemented |
| Style/RedundantFormat                      |   290 | Implemented |
| Style/RedundantHeredocDelimiterQuotes      |    17 | Implemented |
| Style/RedundantInitialize                  |    23 | Implemented |
| Style/RedundantInterpolationUnfreeze       |    17 | Implemented |
| Style/RedundantLineContinuation            |   163 | Implemented |
| Style/RedundantMinMaxBy                    |    33 | Implemented |
| Style/RedundantRegexpArgument              |    50 | Implemented |
| Style/RedundantRegexpConstructor           |    10 | Implemented |
| Style/RedundantSelfAssignmentBranch        |    22 | Implemented |
| Style/RedundantStringEscape                |   328 | Implemented |
| Style/RedundantStructKeywordInit           |    17 | Implemented |
| Style/ReturnNilInPredicateMethodDefinition |    39 | Implemented |
| Style/ReverseFind                          |    14 | Implemented |
| Style/SafeNavigationChainLength            |     8 | Implemented |
| Style/SelectByKind                         |   144 | Implemented |
| Style/SelectByRange                        |   120 | Implemented |
| Style/SelectByRegexp                       |   320 | Implemented |
| Style/SendWithLiteralMethodName            |   115 | Implemented |
| Style/SingleLineDoEndBlock                 |    13 | Implemented |
| Style/StringChars                          |     8 | Implemented |
| Style/SuperArguments                       |    92 | Implemented |
| Style/SuperWithArgsParentheses             |     4 | Implemented |
| Style/SwapValues                           |    11 | Implemented |
| Style/TallyMethod                          |    32 | Implemented |
| Style/YAMLFileRead                         |    11 | Implemented |

### Disabled by Default (32 cops, 741 tests)

| Cop                                        | Tests | Status      |
| ------------------------------------------ | ----: | ----------- |
| Style/ArrayCoercion                        |     5 | Implemented |
| Style/ArrayFirstLast                       |    16 | Implemented |
| Style/AsciiComments                        |     5 | Implemented |
| Style/AutoResourceCleanup                  |     7 | Implemented |
| Style/ClassMethodsDefinitions              |    16 | Implemented |
| Style/CollectionMethods                    |    68 | Implemented |
| Style/ConstantVisibility                   |    15 | Implemented |
| Style/Copyright                            |    13 | Implemented |
| Style/DateTime                             |    12 | Implemented |
| Style/DisableCopsWithinSourceCodeDirective |     7 | Implemented |
| Style/DocumentationMethod                  |    77 | Implemented |
| Style/HashLookupMethod                     |    15 | Implemented |
| Style/ImplicitRuntimeError                 |     8 | Implemented |
| Style/InlineComment                        |     3 | Implemented |
| Style/InvertibleUnlessCondition            |    15 | Implemented |
| Style/IpAddresses                          |    14 | Implemented |
| Style/MethodCallWithArgsParentheses        |   174 | Implemented |
| Style/MethodCalledOnDoEndBlock             |    10 | Implemented |
| Style/MissingElse                          |    84 | Implemented |
| Style/MultilineMethodSignature             |    19 | Implemented |
| Style/OptionHash                           |     9 | Implemented |
| Style/RequireOrder                         |    24 | Implemented |
| Style/ReturnNil                            |     5 | Implemented |
| Style/Send                                 |    13 | Implemented |
| Style/SingleLineBlockParams                |    12 | Implemented |
| Style/StaticClass                          |    11 | Implemented |
| Style/StringHashKeys                       |    10 | Implemented |
| Style/StringMethods                        |     2 | Implemented |
| Style/TopLevelMethodDefinition             |    14 | Implemented |
| Style/TrailingCommaInBlockArgs             |    20 | Implemented |
| Style/UnlessLogicalOperators               |    28 | Implemented |
| Style/YodaExpression                       |    10 | Implemented |

## Lint (154/154 implemented, 5,961 tests)

### Enabled by Default (100 cops, 3,859 tests)

| Cop                                      | Tests | Status      |
| ---------------------------------------- | ----: | ----------- |
| Lint/AmbiguousBlockAssociation           |    36 | Implemented |
| Lint/AmbiguousOperator                   |    17 | Implemented |
| Lint/AmbiguousRegexpLiteral              |    30 | Implemented |
| Lint/AssignmentInCondition               |    69 | Implemented |
| Lint/BigDecimalNew                       |     3 | Implemented |
| Lint/BinaryOperatorWithIdenticalOperands |    23 | Implemented |
| Lint/BooleanSymbol                       |    10 | Implemented |
| Lint/CircularArgumentReference           |    13 | Implemented |
| Lint/ConstantDefinitionInBlock           |    27 | Implemented |
| Lint/Debugger                            |    97 | Implemented |
| Lint/DeprecatedClassMethods              |    31 | Implemented |
| Lint/DeprecatedOpenSSLConstant           |    24 | Implemented |
| Lint/DisjunctiveAssignmentInConstructor  |     7 | Implemented |
| Lint/DuplicateCaseCondition              |     9 | Implemented |
| Lint/DuplicateElsifCondition             |     5 | Implemented |
| Lint/DuplicateHashKey                    |    33 | Implemented |
| Lint/DuplicateMethods                    |   329 | Implemented |
| Lint/DuplicateRequire                    |    10 | Implemented |
| Lint/DuplicateRescueException            |     6 | Implemented |
| Lint/EachWithObjectArgument              |     7 | Implemented |
| Lint/ElseLayout                          |    12 | Implemented |
| Lint/EmptyConditionalBody                |    42 | Implemented |
| Lint/EmptyEnsure                         |     2 | Implemented |
| Lint/EmptyExpression                     |    12 | Implemented |
| Lint/EmptyFile                           |     2 | Implemented |
| Lint/EmptyInterpolation                  |    12 | Implemented |
| Lint/EmptyWhen                           |    16 | Implemented |
| Lint/EnsureReturn                        |     5 | Implemented |
| Lint/ErbNewArguments                     |    10 | Implemented |
| Lint/FlipFlop                            |     2 | Implemented |
| Lint/FloatComparison                     |    17 | Implemented |
| Lint/FloatOutOfRange                     |     5 | Implemented |
| Lint/FormatParameterMismatch             |    75 | Implemented |
| Lint/HashCompareByIdentity               |     4 | Implemented |
| Lint/IdentityComparison                  |    12 | Implemented |
| Lint/ImplicitStringConcatenation         |    12 | Implemented |
| Lint/IneffectiveAccessModifier           |     8 | Implemented |
| Lint/InheritException                    |    13 | Implemented |
| Lint/InterpolationCheck                  |    15 | Implemented |
| Lint/LiteralAsCondition                  |   229 | Implemented |
| Lint/LiteralInInterpolation              |   378 | Implemented |
| Lint/Loop                                |     4 | Implemented |
| Lint/MissingCopEnableDirective           |    11 | Implemented |
| Lint/MissingSuper                        |    22 | Implemented |
| Lint/MixedRegexpCaptureTypes             |    12 | Implemented |
| Lint/MultipleComparison                  |    20 | Implemented |
| Lint/NestedMethodDefinition              |    38 | Implemented |
| Lint/NestedPercentLiteral                |    11 | Implemented |
| Lint/NextWithoutAccumulator              |    18 | Implemented |
| Lint/NonDeterministicRequireOrder        |    28 | Implemented |
| Lint/NonLocalExitFromIterator            |    14 | Implemented |
| Lint/OrderedMagicComments                |    10 | Implemented |
| Lint/OutOfRangeRegexpRef                 |   122 | Implemented |
| Lint/ParenthesesAsGroupedExpression      |    26 | Implemented |
| Lint/PercentStringArray                  |    22 | Implemented |
| Lint/PercentSymbolArray                  |    12 | Implemented |
| Lint/RaiseException                      |    15 | Implemented |
| Lint/RandOne                             |    16 | Implemented |
| Lint/RedundantCopDisableDirective        |    44 | Implemented |
| Lint/RedundantCopEnableDirective         |    23 | Implemented |
| Lint/RedundantRequireStatement           |    15 | Implemented |
| Lint/RedundantSafeNavigation             |    72 | Implemented |
| Lint/RedundantSplatExpansion             |    59 | Implemented |
| Lint/RedundantStringCoercion             |    18 | Implemented |
| Lint/RedundantWithIndex                  |    17 | Implemented |
| Lint/RedundantWithObject                 |    14 | Implemented |
| Lint/RegexpAsCondition                   |     5 | Implemented |
| Lint/RequireParentheses                  |    16 | Implemented |
| Lint/RescueException                     |    11 | Implemented |
| Lint/RescueType                          |    52 | Implemented |
| Lint/ReturnInVoidContext                 |    18 | Implemented |
| Lint/SafeNavigationChain                 |    63 | Implemented |
| Lint/SafeNavigationConsistency           |    43 | Implemented |
| Lint/SafeNavigationWithEmpty             |     3 | Implemented |
| Lint/ScriptPermission                    |     6 | Implemented |
| Lint/SelfAssignment                      |    58 | Implemented |
| Lint/SendWithMixinArgument               |    14 | Implemented |
| Lint/ShadowedArgument                    |    54 | Implemented |
| Lint/ShadowedException                   |    38 | Implemented |
| Lint/StructNewOverride                   |    10 | Implemented |
| Lint/SuppressedException                 |    24 | Implemented |
| Lint/Syntax                              |     0 | Implemented |
| Lint/ToJSON                              |     2 | Implemented |
| Lint/TopLevelReturnWithArgument          |    10 | Implemented |
| Lint/TrailingCommaInAttributeDeclaration |     2 | Implemented |
| Lint/UnderscorePrefixedVariableName      |    19 | Implemented |
| Lint/UnifiedInteger                      |    15 | Implemented |
| Lint/UnreachableCode                     |   266 | Implemented |
| Lint/UnreachableLoop                     |    28 | Implemented |
| Lint/UnusedBlockArgument                 |    30 | Implemented |
| Lint/UnusedMethodArgument                |    41 | Implemented |
| Lint/UriEscapeUnescape                   |     9 | Implemented |
| Lint/UriRegexp                           |    10 | Implemented |
| Lint/UselessAccessModifier               |   198 | Implemented |
| Lint/UselessAssignment                   |   149 | Implemented |
| Lint/UselessElseWithoutRescue            |     2 | Implemented |
| Lint/UselessMethodDefinition             |    16 | Implemented |
| Lint/UselessSetterCall                   |    20 | Implemented |
| Lint/UselessTimes                        |    25 | Implemented |
| Lint/Void                                |   270 | Implemented |

### Pending by Default (50 cops, 2,007 tests)

| Cop                                         | Tests | Status      |
| ------------------------------------------- | ----: | ----------- |
| Lint/AmbiguousAssignment                    |    40 | Implemented |
| Lint/AmbiguousOperatorPrecedence            |    13 | Implemented |
| Lint/AmbiguousRange                         |    54 | Implemented |
| Lint/ArrayLiteralInRegexp                   |    32 | Implemented |
| Lint/ConstantOverwrittenInRescue            |     8 | Implemented |
| Lint/ConstantReassignment                   |    41 | Implemented |
| Lint/CopDirectiveSyntax                     |    16 | Implemented |
| Lint/DataDefineOverride                     |     8 | Implemented |
| Lint/DeprecatedConstants                    |    20 | Implemented |
| Lint/DuplicateBranch                        |   131 | Implemented |
| Lint/DuplicateMagicComment                  |     8 | Implemented |
| Lint/DuplicateMatchPattern                  |    19 | Implemented |
| Lint/DuplicateRegexpCharacterClassElement   |    16 | Implemented |
| Lint/DuplicateSetElement                    |    36 | Implemented |
| Lint/EmptyBlock                             |    17 | Implemented |
| Lint/EmptyClass                             |     9 | Implemented |
| Lint/EmptyInPattern                         |    13 | Implemented |
| Lint/HashNewWithKeywordArgumentsAsDefault   |    10 | Implemented |
| Lint/IncompatibleIoSelectWithFiberScheduler |    19 | Implemented |
| Lint/ItWithoutArgumentsInBlock              |    19 | Implemented |
| Lint/LambdaWithoutLiteralBlock              |     6 | Implemented |
| Lint/LiteralAssignmentInCondition           |    34 | Implemented |
| Lint/MixedCaseRange                         |    31 | Implemented |
| Lint/NoReturnInBeginEndBlocks               |    70 | Implemented |
| Lint/NonAtomicFileOperation                 |    43 | Implemented |
| Lint/NumberedParameterAssignment            |    13 | Implemented |
| Lint/NumericOperationWithConstantResult     |    16 | Implemented |
| Lint/OrAssignmentToConstant                 |    10 | Implemented |
| Lint/RedundantDirGlobSort                   |    16 | Implemented |
| Lint/RedundantRegexpQuantifiers             |    26 | Implemented |
| Lint/RedundantTypeConversion                |   613 | Implemented |
| Lint/RefinementImportMethods                |     7 | Implemented |
| Lint/RequireRangeParentheses                |     9 | Implemented |
| Lint/RequireRelativeSelfPath                |     6 | Implemented |
| Lint/SharedMutableDefault                   |     6 | Implemented |
| Lint/SuppressedExceptionInNumberConversion  |    26 | Implemented |
| Lint/SymbolConversion                       |    39 | Implemented |
| Lint/ToEnumArguments                        |    24 | Implemented |
| Lint/TripleQuotes                           |     9 | Implemented |
| Lint/UnescapedBracketInRegexp               |    44 | Implemented |
| Lint/UnexpectedBlockArity                   |    22 | Implemented |
| Lint/UnmodifiedReduceAccumulator            |   168 | Implemented |
| Lint/UnreachablePatternBranch               |    23 | Implemented |
| Lint/UselessConstantScoping                 |    11 | Implemented |
| Lint/UselessDefaultValueArgument            |    24 | Implemented |
| Lint/UselessDefined                         |     7 | Implemented |
| Lint/UselessNumericOperation                |    13 | Implemented |
| Lint/UselessOr                              |   127 | Implemented |
| Lint/UselessRescue                          |    12 | Implemented |
| Lint/UselessRuby2Keywords                   |    23 | Implemented |

### Disabled by Default (4 cops, 95 tests)

| Cop                              | Tests | Status |
| -------------------------------- | ----: | ------ |
| Lint/ConstantResolution          |    18 | -      |
| Lint/HeredocMethodCallPosition   |    10 | -      |
| Lint/NumberConversion            |    36 | -      |
| Lint/ShadowingOuterLocalVariable |    31 | -      |

## Layout (100/100 implemented, 4,654 tests)

### Enabled by Default (81 cops, 4,067 tests)

| Cop                                              | Tests | Status      |
| ------------------------------------------------ | ----: | ----------- |
| Layout/AccessModifierIndentation                 |    43 | Implemented |
| Layout/ArgumentAlignment                         |    52 | Implemented |
| Layout/ArrayAlignment                            |    25 | Implemented |
| Layout/AssignmentIndentation                     |    10 | Implemented |
| Layout/BeginEndAlignment                         |     7 | Implemented |
| Layout/BlockAlignment                            |    78 | Implemented |
| Layout/BlockEndNewline                           |    18 | Implemented |
| Layout/CaseIndentation                           |    50 | Implemented |
| Layout/ClosingHeredocIndentation                 |    11 | Implemented |
| Layout/ClosingParenthesisIndentation             |    43 | Implemented |
| Layout/CommentIndentation                        |    28 | Implemented |
| Layout/ConditionPosition                         |    14 | Implemented |
| Layout/DefEndAlignment                           |    18 | Implemented |
| Layout/DotPosition                               |    39 | Implemented |
| Layout/ElseAlignment                             |    52 | Implemented |
| Layout/EmptyComment                              |    14 | Implemented |
| Layout/EmptyLineAfterGuardClause                 |    47 | Implemented |
| Layout/EmptyLineAfterMagicComment                |    21 | Implemented |
| Layout/EmptyLineBetweenDefs                      |    45 | Implemented |
| Layout/EmptyLines                                |     5 | Implemented |
| Layout/EmptyLinesAroundAccessModifier            |   176 | Implemented |
| Layout/EmptyLinesAroundArguments                 |    22 | Implemented |
| Layout/EmptyLinesAroundAttributeAccessor         |    20 | Implemented |
| Layout/EmptyLinesAroundBeginBody                 |    11 | Implemented |
| Layout/EmptyLinesAroundBlockBody                 |    20 | Implemented |
| Layout/EmptyLinesAroundClassBody                 |    46 | Implemented |
| Layout/EmptyLinesAroundExceptionHandlingKeywords |    24 | Implemented |
| Layout/EmptyLinesAroundMethodBody                |    14 | Implemented |
| Layout/EmptyLinesAroundModuleBody                |    38 | Implemented |
| Layout/EndAlignment                              |   207 | Implemented |
| Layout/EndOfLine                                 |    13 | Implemented |
| Layout/ExtraSpacing                              |    82 | Implemented |
| Layout/FirstArgumentIndentation                  |   139 | Implemented |
| Layout/FirstArrayElementIndentation              |    53 | Implemented |
| Layout/FirstHashElementIndentation               |    60 | Implemented |
| Layout/FirstParameterIndentation                 |    20 | Implemented |
| Layout/HashAlignment                             |   131 | Implemented |
| Layout/HeredocIndentation                        |   105 | Implemented |
| Layout/IndentationConsistency                    |    53 | Implemented |
| Layout/IndentationStyle                          |    25 | Implemented |
| Layout/IndentationWidth                          |   177 | Implemented |
| Layout/InitialIndentation                        |     8 | Implemented |
| Layout/LeadingCommentSpace                       |    27 | Implemented |
| Layout/LeadingEmptyLines                         |     9 | Implemented |
| Layout/LineLength                                |   192 | Implemented |
| Layout/MultilineArrayBraceLayout                 |    35 | Implemented |
| Layout/MultilineBlockLayout                      |    30 | Implemented |
| Layout/MultilineHashBraceLayout                  |    34 | Implemented |
| Layout/MultilineMethodCallBraceLayout            |    44 | Implemented |
| Layout/MultilineMethodCallIndentation            |   252 | Implemented |
| Layout/MultilineMethodDefinitionBraceLayout      |    26 | Implemented |
| Layout/MultilineOperationIndentation             |   101 | Implemented |
| Layout/ParameterAlignment                        |    19 | Implemented |
| Layout/RescueEnsureAlignment                     |    99 | Implemented |
| Layout/SpaceAfterColon                           |    12 | Implemented |
| Layout/SpaceAfterComma                           |     9 | Implemented |
| Layout/SpaceAfterMethodName                      |     8 | Implemented |
| Layout/SpaceAfterNot                             |     6 | Implemented |
| Layout/SpaceAfterSemicolon                       |     9 | Implemented |
| Layout/SpaceAroundBlockParameters                |    45 | Implemented |
| Layout/SpaceAroundEqualsInParameterDefault       |    11 | Implemented |
| Layout/SpaceAroundKeyword                        |   112 | Implemented |
| Layout/SpaceAroundMethodCallOperator             |    51 | Implemented |
| Layout/SpaceAroundOperators                      |    99 | Implemented |
| Layout/SpaceBeforeBlockBraces                    |    18 | Implemented |
| Layout/SpaceBeforeComma                          |     6 | Implemented |
| Layout/SpaceBeforeComment                        |     5 | Implemented |
| Layout/SpaceBeforeFirstArg                       |    12 | Implemented |
| Layout/SpaceBeforeSemicolon                      |     9 | Implemented |
| Layout/SpaceInLambdaLiteral                      |    15 | Implemented |
| Layout/SpaceInsideArrayLiteralBrackets           |    99 | Implemented |
| Layout/SpaceInsideArrayPercentLiteral            |   129 | Implemented |
| Layout/SpaceInsideBlockBraces                    |    43 | Implemented |
| Layout/SpaceInsideHashLiteralBraces              |    40 | Implemented |
| Layout/SpaceInsideParens                         |    28 | Implemented |
| Layout/SpaceInsidePercentLiteralDelimiters       |   262 | Implemented |
| Layout/SpaceInsideRangeLiteral                   |     7 | Implemented |
| Layout/SpaceInsideReferenceBrackets              |    47 | Implemented |
| Layout/SpaceInsideStringInterpolation            |    12 | Implemented |
| Layout/TrailingEmptyLines                        |    18 | Implemented |
| Layout/TrailingWhitespace                        |    19 | Implemented |

### Pending by Default (5 cops, 209 tests)

| Cop                                          | Tests | Status      |
| -------------------------------------------- | ----: | ----------- |
| Layout/EmptyLinesAfterModuleInclusion        |    59 | Implemented |
| Layout/LineContinuationLeadingSpace          |    32 | Implemented |
| Layout/LineContinuationSpacing               |    31 | Implemented |
| Layout/LineEndStringConcatenationIndentation |    59 | Implemented |
| Layout/SpaceBeforeBrackets                   |    28 | Implemented |

### Disabled by Default (14 cops, 378 tests)

| Cop                                       | Tests | Status      |
| ----------------------------------------- | ----: | ----------- |
| Layout/ClassStructure                     |    21 | Implemented |
| Layout/EmptyLineAfterMultilineCondition   |    22 | Implemented |
| Layout/FirstArrayElementLineBreak         |    14 | Implemented |
| Layout/FirstHashElementLineBreak          |    11 | Implemented |
| Layout/FirstMethodArgumentLineBreak       |    14 | Implemented |
| Layout/FirstMethodParameterLineBreak      |    11 | Implemented |
| Layout/HeredocArgumentClosingParenthesis  |    82 | Implemented |
| Layout/MultilineArrayLineBreaks           |     6 | Implemented |
| Layout/MultilineAssignmentLayout          |    34 | Implemented |
| Layout/MultilineHashKeyLineBreaks         |    10 | Implemented |
| Layout/MultilineMethodArgumentLineBreaks  |    18 | Implemented |
| Layout/MultilineMethodParameterLineBreaks |    14 | Implemented |
| Layout/RedundantLineBreak                 |   112 | Implemented |
| Layout/SingleLineBlockChain               |     9 | Implemented |

## Metrics (9/10 implemented, 272 tests)

### Enabled by Default (9 cops, 259 tests)

| Cop                          | Tests | Status      |
| ---------------------------- | ----: | ----------- |
| Metrics/AbcSize              |    25 | Implemented |
| Metrics/BlockLength          |    38 | Implemented |
| Metrics/BlockNesting         |    26 | Implemented |
| Metrics/ClassLength          |    34 | Implemented |
| Metrics/CyclomaticComplexity |    37 | Implemented |
| Metrics/MethodLength         |    31 | Implemented |
| Metrics/ModuleLength         |    21 | Implemented |
| Metrics/ParameterLists       |    16 | Implemented |
| Metrics/PerceivedComplexity  |    31 | Implemented |

### Pending by Default (1 cops, 13 tests)

| Cop                             | Tests | Status      |
| ------------------------------- | ----: | ----------- |
| Metrics/CollectionLiteralLength |    13 | Implemented |

## Naming (17/19 implemented, 2,217 tests)

### Enabled by Default (16 cops, 884 tests)

| Cop                                  | Tests | Status      |
| ------------------------------------ | ----: | ----------- |
| Naming/AccessorMethodName            |    23 | Implemented |
| Naming/AsciiIdentifiers              |    12 | Implemented |
| Naming/BinaryOperatorParameterName   |    15 | Implemented |
| Naming/BlockParameterName            |    13 | Implemented |
| Naming/ClassAndModuleCamelCase       |     5 | Implemented |
| Naming/ConstantName                  |    24 | Implemented |
| Naming/FileName                      |   120 | Implemented |
| Naming/HeredocDelimiterCase          |    26 | Implemented |
| Naming/HeredocDelimiterNaming        |    19 | Implemented |
| Naming/MemoizedInstanceVariableName  |    72 | Implemented |
| Naming/MethodName                    |   239 | Implemented |
| Naming/MethodParameterName           |    23 | Implemented |
| Naming/PredicatePrefix               |    24 | Implemented |
| Naming/RescuedExceptionsVariableName |    36 | Implemented |
| Naming/VariableName                  |   118 | Implemented |
| Naming/VariableNumber                |   115 | Implemented |

### Pending by Default (2 cops, 1,298 tests)

| Cop                    | Tests | Status      |
| ---------------------- | ----: | ----------- |
| Naming/BlockForwarding |    36 | Implemented |
| Naming/PredicateMethod |  1262 | Implemented |

### Disabled by Default (1 cops, 35 tests)

| Cop                      | Tests | Status      |
| ------------------------ | ----: | ----------- |
| Naming/InclusiveLanguage |    35 | Implemented |

## Gemspec (4/10 implemented, 193 tests)

### Enabled by Default (4 cops, 61 tests)

| Cop                             | Tests | Status      |
| ------------------------------- | ----: | ----------- |
| Gemspec/DuplicatedAssignment    |    17 | Implemented |
| Gemspec/OrderedDependencies     |    18 | Implemented |
| Gemspec/RequiredRubyVersion     |    21 | Implemented |
| Gemspec/RubyVersionGlobalsUsage |     5 | Implemented |

### Pending by Default (5 cops, 55 tests)

| Cop                                   | Tests | Status      |
| ------------------------------------- | ----: | ----------- |
| Gemspec/AddRuntimeDependency          |     5 | Implemented |
| Gemspec/AttributeAssignment           |     7 | Implemented |
| Gemspec/DeprecatedAttributeAssignment |    18 | Implemented |
| Gemspec/DevelopmentDependencies       |    13 | Implemented |
| Gemspec/RequireMFA                    |    12 | Implemented |

### Disabled by Default (1 cops, 77 tests)

| Cop                       | Tests | Status      |
| ------------------------- | ----: | ----------- |
| Gemspec/DependencyVersion |    77 | Implemented |

## Bundler (5/7 implemented, 101 tests)

### Enabled by Default (5 cops, 69 tests)

| Cop                            | Tests | Status      |
| ------------------------------ | ----: | ----------- |
| Bundler/DuplicatedGem          |    10 | Implemented |
| Bundler/DuplicatedGroup        |    21 | Implemented |
| Bundler/GemFilename            |    15 | Implemented |
| Bundler/InsecureProtocolSource |     6 | Implemented |
| Bundler/OrderedGems            |    17 | Implemented |

### Disabled by Default (2 cops, 32 tests)

| Cop                | Tests | Status      |
| ------------------ | ----: | ----------- |
| Bundler/GemComment |    26 | Implemented |
| Bundler/GemVersion |     6 | Implemented |

## Security (5/7 implemented, 102 tests)

### Enabled by Default (5 cops, 49 tests)

| Cop                  | Tests | Status      |
| -------------------- | ----: | ----------- |
| Security/Eval        |    15 | Implemented |
| Security/JSONLoad    |     7 | Implemented |
| Security/MarshalLoad |     5 | Implemented |
| Security/Open        |    16 | Implemented |
| Security/YAMLLoad    |     6 | Implemented |

### Pending by Default (2 cops, 53 tests)

| Cop                   | Tests | Status      |
| --------------------- | ----: | ----------- |
| Security/CompoundHash |    21 | Implemented |
| Security/IoMethods    |    32 | Implemented |

## Migration (1/1 implemented, 8 tests)

### Enabled by Default (1 cops, 8 tests)

| Cop                      | Tests | Status      |
| ------------------------ | ----: | ----------- |
| Migration/DepartmentName |     8 | Implemented |

## Implementation Clusters (Pending by Default)

149 cops / ~5,440 tests across 23 clusters. Pending-by-default cops run only with `NewCops: enable`. Order = highest test count first within each cluster.

**Completed clusters (3):** Redundant/Useless (20/20), Enumerable transform (7/7), Method def/params (10/10). All previously deferred cops cleared.

| Cluster              | Cops | Tests | Status                                                                      | Notes                                                                                   |
| -------------------- | ---: | ----: | :-------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Misc                 |   44 |  1304 | todo                                                                        | Loose group; subdivide before clustering. Includes UnmodifiedReduceAccumulator etc      |
| Redundant/Useless    |   20 |   843 | **20/20** ✅                                                                | Detect noop call/literal, replace/remove                                                |
| Enumerable transform |    7 |   435 | **7/7** ✅                                                                  | `select`/`reject`/`map` rewrites — share Enumerable matchers w/ existing SelectByRegexp |
| Method def/params    |   10 |   422 | **10/10** ✅                                                                | `it`-block, numbered params, BlockForwarding, ArgumentsForwarding — forwarding helper   |
| Hash transform       |    9 |   408 | **next**                                                                    | `Hash#slice`/`#except`, `to_h` chains — share HashTransformMethod-style matchers        |
| Send/operator        |    2 |   317 | OperatorMethodCall + SendWithLiteralMethodName                              |
| Useless ops          |    7 |   217 | Generic dead-code checks — case-by-case                                     |
| Duplicate detection  |    4 |   194 | `==`-based branch/element/pattern dedup — generic equivalence helper        |
| Empty constructs     |    6 |   166 | EmptyClass / EmptyBlock / EmptyInPattern — body emptiness checks            |
| File ops             |    8 |   147 | `File.read`/`File.write`/`Dir.empty?` shorthand — message-receiver matchers |
| Line layout          |    3 |   122 | Line-continuation (`\`) layout — share continuation-comment scanner         |
| Regexp               |    4 |   106 | Regexp literal scan — port shared regexp tokenizer                          |
| Predicate            |    2 |   103 | PredicateWithKind + ReturnNilInPredicateMethodDefinition                    |
| Constants            |    5 |    95 | Constant reassignment / deprecated lookups                                  |
| Comparison           |    4 |    73 | `Comparable` rewrites — `clamp`/`between?`                                  |
| Ambiguous detection  |    2 |    67 | AmbiguousRange + AmbiguousOperatorPrecedence                                |
| Env                  |    2 |    50 | `ENV[...]` patterns                                                         |
| Pattern matching     |    3 |    43 | `in`/`case in` pattern lints                                                |
| Lambda/proc          |    2 |    37 | NilLambda + LambdaWithoutLiteralBlock                                       |
| Security             |    1 |    32 | Security/IoMethods                                                          |
| Magic/encoding       |    1 |    25 | Style/MagicCommentFormat                                                    |
| Lint misc            |    2 |    21 | OpenStructUse + TripleQuotes                                                |
| Heredoc              |    1 |     7 | Style/EmptyHeredoc                                                          |

Cluster details (cop name + test count):

### Misc — 44 cops, 1304 tests

Loose grab-bag — subdivide on next pass.

- Lint/UnmodifiedReduceAccumulator (168), Style/ModuleMemberExistenceCheck (101), Style/QuotedSymbols (97), Style/IfWithBooleanLiteralBranches (94), Style/SuperArguments (92), Lint/NoReturnInBeginEndBlocks (70), Lint/NonAtomicFileOperation (43), Style/CombinableDefined (39), Style/PartitionInsteadOfDoubleSelect (37), Lint/LiteralAssignmentInCondition (34), Style/TallyMethod (32), Style/NegatedIfElseCondition (32), Lint/MixedCaseRange (31), Layout/SpaceBeforeBrackets (28), Lint/SuppressedExceptionInNumberConversion (26), Lint/ToEnumArguments (24), Style/DataInheritance (24), Style/DigChain (23), Style/ObjectThen (23), Lint/UnexpectedBlockArity (22), Style/OneClassPerFile (21), Lint/IncompatibleIoSelectWithFiberScheduler (19), Style/DocumentDynamicEvalDefinition (18), Gemspec/DeprecatedAttributeAssignment (18), Lint/CopDirectiveSyntax (16), Style/ReverseFind (14), Style/ConcatArrayLiterals (14), Metrics/CollectionLiteralLength (13), Style/SingleLineDoEndBlock (13), Gemspec/DevelopmentDependencies (13), Gemspec/RequireMFA (12), Style/SwapValues (11), Lint/RequireRangeParentheses (9), Style/KeywordArgumentsMerging (9), Lint/DataDefineOverride (8), Style/StringChars (8), Style/SafeNavigationChainLength (8), Lint/RefinementImportMethods (7), Gemspec/AttributeAssignment (7), Lint/RequireRelativeSelfPath (6), Lint/SharedMutableDefault (6), Style/NestedFileDirname (5), Gemspec/AddRuntimeDependency (5), Style/SuperWithArgsParentheses (4)

### Redundant/Useless — 20/20 ✅

Implemented: RedundantFormat (290), RedundantLineContinuation (163), RedundantRegexpArgument (50), RedundantFilterChain (39), RedundantMinMaxBy (33), RedundantEach (33), RedundantDoubleSplatHashBraces (29), RedundantRegexpQuantifiers (26), RedundantInitialize (23), RedundantSelfAssignmentBranch (22), RedundantHeredocDelimiterQuotes (17), RedundantInterpolationUnfreeze (17), RedundantStructKeywordInit (17), RedundantDirGlobSort (16), RedundantArgument (15), RedundantArrayConstructor (13), RedundantCurrentDirectoryInPath (12), RedundantRegexpConstructor (10), RedundantArrayFlatten (10), RedundantConstantBase (8)

### Enumerable transform — 7/7 ✅

Implemented: SelectByKind (144), SelectByRange (120), MapIntoArray (64), MapCompactWithConditionalBlock (33), CollectionCompact (30), MapJoin (24), CollectionQuerying (20)

### Method def/params — 10/10 ✅

Implemented: ArgumentsForwarding (187), EndlessMethod (63), BlockForwarding (36), ItBlockParameter (34), AmbiguousEndlessMethodDefinition (31), ItAssignment (23), ItWithoutArgumentsInBlock (19), NumberedParameterAssignment (13), NumberedParametersLimit (12), NumberedParameters (4)

### Hash transform — 9 cops, 408 tests

- Style/HashSlice (116), Style/HashExcept (114), Style/MapToHash (38), Style/HashFetchChain (35), Style/MapToSet (32), Style/HashConversion (22), Security/CompoundHash (21), Style/ReduceToHash (20), Lint/HashNewWithKeywordArgumentsAsDefault (10)

### Send/operator — 2 cops, 317 tests

- Style/OperatorMethodCall (202), Style/SendWithLiteralMethodName (115)

### Useless ops — 7 cops, 217 tests

- Lint/UselessOr (127), Lint/UselessDefaultValueArgument (24), Lint/UselessRuby2Keywords (23), Lint/UselessNumericOperation (13), Lint/UselessRescue (12), Lint/UselessConstantScoping (11), Lint/UselessDefined (7)

### Duplicate detection — 4 cops, 194 tests

- Lint/DuplicateBranch (131), Lint/DuplicateSetElement (36), Lint/DuplicateMatchPattern (19), Lint/DuplicateMagicComment (8)

### Empty constructs — 6 cops, 166 tests

- Layout/EmptyLinesAfterModuleInclusion (59), Style/EmptyClassDefinition (44), Style/EmptyStringInsideInterpolation (24), Lint/EmptyBlock (17), Lint/EmptyInPattern (13), Lint/EmptyClass (9)

### File ops — 8 cops, 147 tests

- Style/FileWrite (32), Style/FileRead (30), Style/FileEmpty (27), Style/DirEmpty (16), Style/FileOpen (14), Style/FileNull (13), Style/YAMLFileRead (11), Style/FileTouch (4)

### Line layout — 3 cops, 122 tests

- Layout/LineEndStringConcatenationIndentation (59), Layout/LineContinuationLeadingSpace (32), Layout/LineContinuationSpacing (31)

### Regexp — 4 cops, 106 tests

- Lint/UnescapedBracketInRegexp (44), Lint/ArrayLiteralInRegexp (32), Lint/DuplicateRegexpCharacterClassElement (16), Style/ExactRegexpMatch (14)

### Predicate — 2 cops, 103 tests

- Style/PredicateWithKind (64), Style/ReturnNilInPredicateMethodDefinition (39)

### Constants — 5 cops, 95 tests

- Lint/ConstantReassignment (41), Lint/DeprecatedConstants (20), Lint/NumericOperationWithConstantResult (16), Lint/OrAssignmentToConstant (10), Lint/ConstantOverwrittenInRescue (8)

### Comparison — 4 cops, 73 tests

- Style/ComparableClamp (23), Style/BitwisePredicate (18), Style/MinMaxComparison (17), Style/ComparableBetween (15)

### Ambiguous detection — 2 cops, 67 tests

- Lint/AmbiguousRange (54), Lint/AmbiguousOperatorPrecedence (13)

### Env — 2 cops, 50 tests

- Style/FetchEnvVar (43), Style/EnvHome (7)

### Pattern matching — 3 cops, 43 tests

- Lint/UnreachablePatternBranch (23), Style/MultilineInPatternThen (13), Style/InPatternThen (7)

### Lambda/proc — 2 cops, 37 tests

- Style/NilLambda (31), Lint/LambdaWithoutLiteralBlock (6)

### Security — 1 cop, 32 tests

- Security/IoMethods (32)

### Magic/encoding — 1 cop, 25 tests

- Style/MagicCommentFormat (25)

### Lint misc — 2 cops, 21 tests

- Style/OpenStructUse (12), Lint/TripleQuotes (9)

### Heredoc — 1 cop, 7 tests

- Style/EmptyHeredoc (7)
