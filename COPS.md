# All Cops State (606 total)

Full list of all RuboCop cops tracked by ruby-fast-cop, organized by department and default status.
37 of 606 implemented. See [README.md](README.md) for the implementation roadmap.

## Summary

| Department | Enabled | Pending | Disabled | Implemented |      Tests |
| ---------- | ------: | ------: | -------: | ----------: | ---------: |
| Style      |     175 |      91 |       32 |          19 |     14,567 |
| Lint       |     100 |      50 |        4 |           7 |      5,961 |
| Layout     |      81 |       5 |       14 |           7 |      4,654 |
| Metrics    |       9 |       1 |        0 |           3 |        272 |
| Naming     |      16 |       2 |        1 |           1 |      2,217 |
| Gemspec    |       4 |       5 |        1 |           0 |        193 |
| Bundler    |       5 |       0 |        2 |           0 |        101 |
| Security   |       5 |       2 |        0 |           0 |        102 |
| Migration  |       1 |       0 |        0 |           0 |          8 |
| **Total**  | **396** | **156** |   **54** |      **37** | **28,075** |

- **Enabled**: Runs by default on every codebase (highest priority to implement)
- **Pending**: Runs only with `NewCops: enable` in config
- **Disabled**: Runs only when explicitly enabled in config

## Style (19/298 implemented, 14,567 tests)

### Enabled by Default (175 cops, 9,202 tests)

| Cop                                    | Tests | Status      |
| -------------------------------------- | ----: | ----------- |
| Style/AccessModifierDeclarations       |   377 | Implemented |
| Style/AccessorGrouping                 |    26 | -           |
| Style/Alias                            |    26 | -           |
| Style/AndOr                            |    76 | -           |
| Style/ArrayIntersect                   |    81 | -           |
| Style/ArrayIntersectWithSingleElement  |     3 | -           |
| Style/ArrayJoin                        |     5 | -           |
| Style/Attr                             |    11 | -           |
| Style/BarePercentLiterals              |    36 | -           |
| Style/BeginBlock                       |     1 | -           |
| Style/BisectedAttrAccessor             |    14 | -           |
| Style/BlockComments                    |     5 | -           |
| Style/BlockDelimiters                  |   173 | -           |
| Style/CaseEquality                     |    25 | -           |
| Style/CaseLikeIf                       |    38 | -           |
| Style/CharacterLiteral                 |     5 | -           |
| Style/ClassAndModuleChildren           |    40 | -           |
| Style/ClassCheck                       |     4 | -           |
| Style/ClassEqualityComparison          |    22 | -           |
| Style/ClassMethods                     |     5 | -           |
| Style/ClassVars                        |     5 | -           |
| Style/ColonMethodCall                  |    10 | -           |
| Style/ColonMethodDefinition            |     3 | -           |
| Style/CombinableLoops                  |    20 | -           |
| Style/CommandLiteral                   |    35 | -           |
| Style/CommentAnnotation                |    31 | -           |
| Style/CommentedKeyword                 |    47 | -           |
| Style/ConditionalAssignment            |  1199 | Implemented |
| Style/DefWithParentheses               |     9 | -           |
| Style/Dir                              |     4 | -           |
| Style/Documentation                    |    55 | -           |
| Style/DoubleCopDisableDirective        |     3 | -           |
| Style/DoubleNegation                   |    47 | -           |
| Style/EachForSimpleLoop                |    20 | -           |
| Style/EachWithObject                   |    16 | -           |
| Style/EmptyBlockParameter              |     9 | -           |
| Style/EmptyCaseCondition               |    29 | -           |
| Style/EmptyElse                        |   124 | -           |
| Style/EmptyLambdaParameter             |     3 | -           |
| Style/EmptyLiteral                     |    49 | -           |
| Style/EmptyMethod                      |    32 | -           |
| Style/Encoding                         |    13 | -           |
| Style/EndBlock                         |     2 | -           |
| Style/EvalWithLocation                 |    27 | -           |
| Style/EvenOdd                          |    18 | -           |
| Style/ExpandPathArguments              |    16 | -           |
| Style/ExplicitBlockArgument            |    21 | -           |
| Style/ExponentialNotation              |    27 | -           |
| Style/FloatDivision                    |    31 | -           |
| Style/For                              |    32 | -           |
| Style/FormatString                     |    46 | -           |
| Style/FormatStringToken                |   366 | Implemented |
| Style/FrozenStringLiteralComment       |   107 | Implemented |
| Style/GlobalStdStream                  |     6 | -           |
| Style/GlobalVars                       |    74 | -           |
| Style/GuardClause                      |    91 | -           |
| Style/HashAsLastArrayItem              |    19 | -           |
| Style/HashEachMethods                  |    62 | -           |
| Style/HashLikeCase                     |     8 | -           |
| Style/HashSyntax                       |   189 | Implemented |
| Style/HashTransformKeys                |    40 | -           |
| Style/HashTransformValues              |    40 | -           |
| Style/IdenticalConditionalBranches     |    48 | -           |
| Style/IfInsideElse                     |    21 | -           |
| Style/IfUnlessModifier                 |   126 | -           |
| Style/IfUnlessModifierOfIfUnless       |     7 | -           |
| Style/IfWithSemicolon                  |    28 | -           |
| Style/InfiniteLoop                     |    28 | -           |
| Style/InverseMethods                   |   110 | -           |
| Style/KeywordParametersOrder           |    10 | -           |
| Style/Lambda                           |    38 | -           |
| Style/LambdaCall                       |    19 | -           |
| Style/LineEndConcatenation             |    19 | -           |
| Style/MethodCallWithoutArgsParentheses |    34 | -           |
| Style/MethodDefParentheses             |    49 | -           |
| Style/MinMax                           |    12 | -           |
| Style/MissingRespondToMissing          |     8 | -           |
| Style/MixinGrouping                    |    18 | -           |
| Style/MixinUsage                       |    18 | -           |
| Style/ModuleFunction                   |    11 | -           |
| Style/MultilineBlockChain              |    11 | -           |
| Style/MultilineIfModifier              |    10 | -           |
| Style/MultilineIfThen                  |    11 | -           |
| Style/MultilineMemoization             |    17 | -           |
| Style/MultilineTernaryOperator         |    17 | -           |
| Style/MultilineWhenThen                |    13 | -           |
| Style/MultipleComparison               |    34 | -           |
| Style/MutableConstant                  |   354 | Implemented |
| Style/NegatedIf                        |    15 | -           |
| Style/NegatedUnless                    |    14 | -           |
| Style/NegatedWhile                     |    10 | -           |
| Style/NestedModifier                   |    13 | -           |
| Style/NestedParenthesizedCalls         |    12 | -           |
| Style/NestedTernaryOperator            |     7 | -           |
| Style/Next                             |    72 | -           |
| Style/NilComparison                    |     8 | -           |
| Style/NonNilCheck                      |    21 | -           |
| Style/Not                              |     9 | -           |
| Style/NumericLiteralPrefix             |    10 | -           |
| Style/NumericLiterals                  |    28 | Implemented |
| Style/NumericPredicate                 |    43 | -           |
| Style/OneLineConditional               |   108 | -           |
| Style/OptionalArguments                |    12 | -           |
| Style/OptionalBooleanParameter         |     8 | -           |
| Style/OrAssignment                     |    25 | -           |
| Style/ParallelAssignment               |    86 | -           |
| Style/ParenthesesAroundCondition       |    30 | -           |
| Style/PercentLiteralDelimiters         |    65 | -           |
| Style/PercentQLiterals                 |    21 | -           |
| Style/PerlBackrefs                     |    14 | -           |
| Style/PreferredHashMethods             |     9 | -           |
| Style/Proc                             |     6 | -           |
| Style/RaiseArgs                        |    35 | Implemented |
| Style/RandomWithOffset                 |    29 | -           |
| Style/RedundantAssignment              |    11 | -           |
| Style/RedundantBegin                   |    63 | -           |
| Style/RedundantCapitalW                |    13 | -           |
| Style/RedundantCondition               |   102 | -           |
| Style/RedundantConditional             |    11 | -           |
| Style/RedundantException               |    30 | -           |
| Style/RedundantFetchBlock              |    15 | -           |
| Style/RedundantFileExtensionInRequire  |     4 | -           |
| Style/RedundantFreeze                  |    62 | -           |
| Style/RedundantInterpolation           |    29 | -           |
| Style/RedundantParentheses             |   331 | Implemented |
| Style/RedundantPercentQ                |    25 | -           |
| Style/RedundantRegexpCharacterClass    |    47 | -           |
| Style/RedundantRegexpEscape            |   217 | -           |
| Style/RedundantReturn                  |    39 | -           |
| Style/RedundantSelf                    |    62 | -           |
| Style/RedundantSelfAssignment          |    14 | -           |
| Style/RedundantSort                    |    50 | -           |
| Style/RedundantSortBy                  |     8 | -           |
| Style/RegexpLiteral                    |    57 | -           |
| Style/RescueModifier                   |    21 | -           |
| Style/RescueStandardError              |    37 | Implemented |
| Style/SafeNavigation                   |   786 | Implemented |
| Style/Sample                           |    82 | -           |
| Style/SelfAssignment                   |   105 | -           |
| Style/Semicolon                        |    33 | Implemented |
| Style/SignalException                  |    27 | -           |
| Style/SingleArgumentDig                |    15 | -           |
| Style/SingleLineMethods                |    16 | -           |
| Style/SlicingWithRange                 |    28 | -           |
| Style/SoleNestedConditional            |    73 | -           |
| Style/SpecialGlobalVars                |    31 | -           |
| Style/StabbyLambdaParentheses          |     9 | -           |
| Style/StderrPuts                       |     5 | -           |
| Style/StringConcatenation              |    30 | -           |
| Style/StringLiterals                   |    58 | Implemented |
| Style/StringLiteralsInInterpolation    |    13 | -           |
| Style/Strip                            |     6 | -           |
| Style/StructInheritance                |    12 | -           |
| Style/SymbolArray                      |    33 | -           |
| Style/SymbolLiteral                    |     4 | -           |
| Style/SymbolProc                       |    83 | -           |
| Style/TernaryParentheses               |    98 | -           |
| Style/TrailingBodyOnClass              |     7 | -           |
| Style/TrailingBodyOnMethodDefinition   |    12 | -           |
| Style/TrailingBodyOnModule             |     7 | -           |
| Style/TrailingCommaInArguments         |   178 | -           |
| Style/TrailingCommaInArrayLiteral      |    48 | -           |
| Style/TrailingCommaInHashLiteral       |    41 | -           |
| Style/TrailingMethodEndStatement       |    10 | -           |
| Style/TrailingUnderscoreVariable       |    58 | -           |
| Style/TrivialAccessors                 |    38 | -           |
| Style/UnlessElse                       |     5 | -           |
| Style/UnpackFirst                      |    11 | -           |
| Style/VariableInterpolation            |     9 | -           |
| Style/WhenThen                         |     4 | -           |
| Style/WhileUntilDo                     |     6 | -           |
| Style/WhileUntilModifier               |    34 | -           |
| Style/WordArray                        |    59 | -           |
| Style/YodaCondition                    |    73 | -           |
| Style/ZeroLengthPredicate              |    68 | -           |

### Pending by Default (91 cops, 4,624 tests)

| Cop                                        | Tests | Status      |
| ------------------------------------------ | ----: | ----------- |
| Style/AmbiguousEndlessMethodDefinition     |    31 | -           |
| Style/ArgumentsForwarding                  |   187 | -           |
| Style/BitwisePredicate                     |    18 | -           |
| Style/CollectionCompact                    |    30 | -           |
| Style/CollectionQuerying                   |    20 | -           |
| Style/CombinableDefined                    |    39 | -           |
| Style/ComparableBetween                    |    15 | -           |
| Style/ComparableClamp                      |    23 | -           |
| Style/ConcatArrayLiterals                  |    14 | -           |
| Style/DataInheritance                      |    24 | -           |
| Style/DigChain                             |    23 | -           |
| Style/DirEmpty                             |    16 | -           |
| Style/DocumentDynamicEvalDefinition        |    18 | -           |
| Style/EmptyClassDefinition                 |    44 | -           |
| Style/EmptyHeredoc                         |     7 | -           |
| Style/EmptyStringInsideInterpolation       |    24 | -           |
| Style/EndlessMethod                        |    63 | -           |
| Style/EnvHome                              |     7 | -           |
| Style/ExactRegexpMatch                     |    14 | -           |
| Style/FetchEnvVar                          |    43 | -           |
| Style/FileEmpty                            |    27 | -           |
| Style/FileNull                             |    13 | -           |
| Style/FileOpen                             |    14 | -           |
| Style/FileRead                             |    30 | -           |
| Style/FileTouch                            |     4 | -           |
| Style/FileWrite                            |    32 | -           |
| Style/HashConversion                       |    22 | -           |
| Style/HashExcept                           |   114 | -           |
| Style/HashFetchChain                       |    35 | -           |
| Style/HashSlice                            |   116 | -           |
| Style/IfWithBooleanLiteralBranches         |    94 | -           |
| Style/InPatternThen                        |     7 | -           |
| Style/ItAssignment                         |    23 | -           |
| Style/ItBlockParameter                     |    34 | -           |
| Style/KeywordArgumentsMerging              |     9 | -           |
| Style/MagicCommentFormat                   |    25 | -           |
| Style/MapCompactWithConditionalBlock       |    33 | -           |
| Style/MapIntoArray                         |    64 | -           |
| Style/MapJoin                              |    24 | -           |
| Style/MapToHash                            |    38 | -           |
| Style/MapToSet                             |    32 | -           |
| Style/MinMaxComparison                     |    17 | -           |
| Style/ModuleMemberExistenceCheck           |   101 | -           |
| Style/MultilineInPatternThen               |    13 | -           |
| Style/NegatedIfElseCondition               |    32 | -           |
| Style/NegativeArrayIndex                   |   423 | Implemented |
| Style/NestedFileDirname                    |     5 | -           |
| Style/NilLambda                            |    31 | -           |
| Style/NumberedParameters                   |     4 | -           |
| Style/NumberedParametersLimit              |    12 | -           |
| Style/ObjectThen                           |    23 | -           |
| Style/OneClassPerFile                      |    21 | -           |
| Style/OpenStructUse                        |    12 | -           |
| Style/OperatorMethodCall                   |   202 | -           |
| Style/PartitionInsteadOfDoubleSelect       |    37 | -           |
| Style/PredicateWithKind                    |    64 | -           |
| Style/QuotedSymbols                        |    97 | -           |
| Style/ReduceToHash                         |    20 | -           |
| Style/RedundantArgument                    |    15 | -           |
| Style/RedundantArrayConstructor            |    13 | -           |
| Style/RedundantArrayFlatten                |    10 | -           |
| Style/RedundantConstantBase                |     8 | -           |
| Style/RedundantCurrentDirectoryInPath      |    12 | -           |
| Style/RedundantDoubleSplatHashBraces       |    29 | -           |
| Style/RedundantEach                        |    33 | -           |
| Style/RedundantFilterChain                 |    39 | -           |
| Style/RedundantFormat                      |   290 | -           |
| Style/RedundantHeredocDelimiterQuotes      |    17 | -           |
| Style/RedundantInitialize                  |    23 | -           |
| Style/RedundantInterpolationUnfreeze       |    17 | -           |
| Style/RedundantLineContinuation            |   163 | -           |
| Style/RedundantMinMaxBy                    |    33 | -           |
| Style/RedundantRegexpArgument              |    50 | -           |
| Style/RedundantRegexpConstructor           |    10 | -           |
| Style/RedundantSelfAssignmentBranch        |    22 | -           |
| Style/RedundantStringEscape                |   328 | Implemented |
| Style/RedundantStructKeywordInit           |    17 | -           |
| Style/ReturnNilInPredicateMethodDefinition |    39 | -           |
| Style/ReverseFind                          |    14 | -           |
| Style/SafeNavigationChainLength            |     8 | -           |
| Style/SelectByKind                         |   144 | -           |
| Style/SelectByRange                        |   120 | -           |
| Style/SelectByRegexp                       |   320 | Implemented |
| Style/SendWithLiteralMethodName            |   115 | -           |
| Style/SingleLineDoEndBlock                 |    13 | -           |
| Style/StringChars                          |     8 | -           |
| Style/SuperArguments                       |    92 | -           |
| Style/SuperWithArgsParentheses             |     4 | -           |
| Style/SwapValues                           |    11 | -           |
| Style/TallyMethod                          |    32 | -           |
| Style/YAMLFileRead                         |    11 | -           |

### Disabled by Default (32 cops, 741 tests)

| Cop                                        | Tests | Status      |
| ------------------------------------------ | ----: | ----------- |
| Style/ArrayCoercion                        |     5 | -           |
| Style/ArrayFirstLast                       |    16 | -           |
| Style/AsciiComments                        |     5 | -           |
| Style/AutoResourceCleanup                  |     7 | Implemented |
| Style/ClassMethodsDefinitions              |    16 | -           |
| Style/CollectionMethods                    |    68 | -           |
| Style/ConstantVisibility                   |    15 | -           |
| Style/Copyright                            |    13 | -           |
| Style/DateTime                             |    12 | -           |
| Style/DisableCopsWithinSourceCodeDirective |     7 | -           |
| Style/DocumentationMethod                  |    77 | -           |
| Style/HashLookupMethod                     |    15 | -           |
| Style/ImplicitRuntimeError                 |     8 | -           |
| Style/InlineComment                        |     3 | -           |
| Style/InvertibleUnlessCondition            |    15 | -           |
| Style/IpAddresses                          |    14 | -           |
| Style/MethodCallWithArgsParentheses        |   174 | -           |
| Style/MethodCalledOnDoEndBlock             |    10 | Implemented |
| Style/MissingElse                          |    84 | -           |
| Style/MultilineMethodSignature             |    19 | -           |
| Style/OptionHash                           |     9 | -           |
| Style/RequireOrder                         |    24 | -           |
| Style/ReturnNil                            |     5 | -           |
| Style/Send                                 |    13 | -           |
| Style/SingleLineBlockParams                |    12 | -           |
| Style/StaticClass                          |    11 | -           |
| Style/StringHashKeys                       |    10 | -           |
| Style/StringMethods                        |     2 | Implemented |
| Style/TopLevelMethodDefinition             |    14 | -           |
| Style/TrailingCommaInBlockArgs             |    20 | -           |
| Style/UnlessLogicalOperators               |    28 | -           |
| Style/YodaExpression                       |    10 | -           |

## Lint (7/154 implemented, 5,961 tests)

### Enabled by Default (100 cops, 3,859 tests)

| Cop                                      | Tests | Status      |
| ---------------------------------------- | ----: | ----------- |
| Lint/AmbiguousBlockAssociation           |    36 | -           |
| Lint/AmbiguousOperator                   |    17 | -           |
| Lint/AmbiguousRegexpLiteral              |    30 | -           |
| Lint/AssignmentInCondition               |    69 | Implemented |
| Lint/BigDecimalNew                       |     3 | -           |
| Lint/BinaryOperatorWithIdenticalOperands |    23 | -           |
| Lint/BooleanSymbol                       |    10 | -           |
| Lint/CircularArgumentReference           |    13 | -           |
| Lint/ConstantDefinitionInBlock           |    27 | -           |
| Lint/Debugger                            |    97 | Implemented |
| Lint/DeprecatedClassMethods              |    31 | -           |
| Lint/DeprecatedOpenSSLConstant           |    24 | -           |
| Lint/DisjunctiveAssignmentInConstructor  |     7 | -           |
| Lint/DuplicateCaseCondition              |     9 | -           |
| Lint/DuplicateElsifCondition             |     5 | -           |
| Lint/DuplicateHashKey                    |    33 | -           |
| Lint/DuplicateMethods                    |   329 | Implemented |
| Lint/DuplicateRequire                    |    10 | -           |
| Lint/DuplicateRescueException            |     6 | -           |
| Lint/EachWithObjectArgument              |     7 | -           |
| Lint/ElseLayout                          |    12 | -           |
| Lint/EmptyConditionalBody                |    42 | -           |
| Lint/EmptyEnsure                         |     2 | -           |
| Lint/EmptyExpression                     |    12 | -           |
| Lint/EmptyFile                           |     2 | -           |
| Lint/EmptyInterpolation                  |    12 | -           |
| Lint/EmptyWhen                           |    16 | -           |
| Lint/EnsureReturn                        |     5 | -           |
| Lint/ErbNewArguments                     |    10 | -           |
| Lint/FlipFlop                            |     2 | -           |
| Lint/FloatComparison                     |    17 | -           |
| Lint/FloatOutOfRange                     |     5 | -           |
| Lint/FormatParameterMismatch             |    75 | -           |
| Lint/HashCompareByIdentity               |     4 | -           |
| Lint/IdentityComparison                  |    12 | -           |
| Lint/ImplicitStringConcatenation         |    12 | -           |
| Lint/IneffectiveAccessModifier           |     8 | -           |
| Lint/InheritException                    |    13 | -           |
| Lint/InterpolationCheck                  |    15 | -           |
| Lint/LiteralAsCondition                  |   229 | -           |
| Lint/LiteralInInterpolation              |   378 | Implemented |
| Lint/Loop                                |     4 | -           |
| Lint/MissingCopEnableDirective           |    11 | -           |
| Lint/MissingSuper                        |    22 | -           |
| Lint/MixedRegexpCaptureTypes             |    12 | -           |
| Lint/MultipleComparison                  |    20 | -           |
| Lint/NestedMethodDefinition              |    38 | -           |
| Lint/NestedPercentLiteral                |    11 | -           |
| Lint/NextWithoutAccumulator              |    18 | -           |
| Lint/NonDeterministicRequireOrder        |    28 | -           |
| Lint/NonLocalExitFromIterator            |    14 | -           |
| Lint/OrderedMagicComments                |    10 | -           |
| Lint/OutOfRangeRegexpRef                 |   122 | -           |
| Lint/ParenthesesAsGroupedExpression      |    26 | -           |
| Lint/PercentStringArray                  |    22 | -           |
| Lint/PercentSymbolArray                  |    12 | -           |
| Lint/RaiseException                      |    15 | -           |
| Lint/RandOne                             |    16 | -           |
| Lint/RedundantCopDisableDirective        |    44 | -           |
| Lint/RedundantCopEnableDirective         |    23 | -           |
| Lint/RedundantRequireStatement           |    15 | -           |
| Lint/RedundantSafeNavigation             |    72 | -           |
| Lint/RedundantSplatExpansion             |    59 | -           |
| Lint/RedundantStringCoercion             |    18 | -           |
| Lint/RedundantWithIndex                  |    17 | -           |
| Lint/RedundantWithObject                 |    14 | -           |
| Lint/RegexpAsCondition                   |     5 | -           |
| Lint/RequireParentheses                  |    16 | -           |
| Lint/RescueException                     |    11 | -           |
| Lint/RescueType                          |    52 | -           |
| Lint/ReturnInVoidContext                 |    18 | -           |
| Lint/SafeNavigationChain                 |    63 | -           |
| Lint/SafeNavigationConsistency           |    43 | -           |
| Lint/SafeNavigationWithEmpty             |     3 | -           |
| Lint/ScriptPermission                    |     6 | -           |
| Lint/SelfAssignment                      |    58 | -           |
| Lint/SendWithMixinArgument               |    14 | -           |
| Lint/ShadowedArgument                    |    54 | -           |
| Lint/ShadowedException                   |    38 | -           |
| Lint/StructNewOverride                   |    10 | -           |
| Lint/SuppressedException                 |    24 | -           |
| Lint/Syntax                              |     0 | -           |
| Lint/ToJSON                              |     2 | -           |
| Lint/TopLevelReturnWithArgument          |    10 | -           |
| Lint/TrailingCommaInAttributeDeclaration |     2 | -           |
| Lint/UnderscorePrefixedVariableName      |    19 | -           |
| Lint/UnifiedInteger                      |    15 | -           |
| Lint/UnreachableCode                     |   266 | Implemented |
| Lint/UnreachableLoop                     |    28 | -           |
| Lint/UnusedBlockArgument                 |    30 | -           |
| Lint/UnusedMethodArgument                |    41 | -           |
| Lint/UriEscapeUnescape                   |     9 | -           |
| Lint/UriRegexp                           |    10 | -           |
| Lint/UselessAccessModifier               |   198 | -           |
| Lint/UselessAssignment                   |   149 | -           |
| Lint/UselessElseWithoutRescue            |     2 | -           |
| Lint/UselessMethodDefinition             |    16 | -           |
| Lint/UselessSetterCall                   |    20 | -           |
| Lint/UselessTimes                        |    25 | -           |
| Lint/Void                                |   270 | Implemented |

### Pending by Default (50 cops, 2,007 tests)

| Cop                                         | Tests | Status      |
| ------------------------------------------- | ----: | ----------- |
| Lint/AmbiguousAssignment                    |    40 | -           |
| Lint/AmbiguousOperatorPrecedence            |    13 | -           |
| Lint/AmbiguousRange                         |    54 | -           |
| Lint/ArrayLiteralInRegexp                   |    32 | -           |
| Lint/ConstantOverwrittenInRescue            |     8 | -           |
| Lint/ConstantReassignment                   |    41 | -           |
| Lint/CopDirectiveSyntax                     |    16 | -           |
| Lint/DataDefineOverride                     |     8 | -           |
| Lint/DeprecatedConstants                    |    20 | -           |
| Lint/DuplicateBranch                        |   131 | -           |
| Lint/DuplicateMagicComment                  |     8 | -           |
| Lint/DuplicateMatchPattern                  |    19 | -           |
| Lint/DuplicateRegexpCharacterClassElement   |    16 | -           |
| Lint/DuplicateSetElement                    |    36 | -           |
| Lint/EmptyBlock                             |    17 | -           |
| Lint/EmptyClass                             |     9 | -           |
| Lint/EmptyInPattern                         |    13 | -           |
| Lint/HashNewWithKeywordArgumentsAsDefault   |    10 | -           |
| Lint/IncompatibleIoSelectWithFiberScheduler |    19 | -           |
| Lint/ItWithoutArgumentsInBlock              |    19 | -           |
| Lint/LambdaWithoutLiteralBlock              |     6 | -           |
| Lint/LiteralAssignmentInCondition           |    34 | -           |
| Lint/MixedCaseRange                         |    31 | -           |
| Lint/NoReturnInBeginEndBlocks               |    70 | -           |
| Lint/NonAtomicFileOperation                 |    43 | -           |
| Lint/NumberedParameterAssignment            |    13 | -           |
| Lint/NumericOperationWithConstantResult     |    16 | -           |
| Lint/OrAssignmentToConstant                 |    10 | -           |
| Lint/RedundantDirGlobSort                   |    16 | -           |
| Lint/RedundantRegexpQuantifiers             |    26 | -           |
| Lint/RedundantTypeConversion                |   613 | Implemented |
| Lint/RefinementImportMethods                |     7 | -           |
| Lint/RequireRangeParentheses                |     9 | -           |
| Lint/RequireRelativeSelfPath                |     6 | -           |
| Lint/SharedMutableDefault                   |     6 | -           |
| Lint/SuppressedExceptionInNumberConversion  |    26 | -           |
| Lint/SymbolConversion                       |    39 | -           |
| Lint/ToEnumArguments                        |    24 | -           |
| Lint/TripleQuotes                           |     9 | -           |
| Lint/UnescapedBracketInRegexp               |    44 | -           |
| Lint/UnexpectedBlockArity                   |    22 | -           |
| Lint/UnmodifiedReduceAccumulator            |   168 | -           |
| Lint/UnreachablePatternBranch               |    23 | -           |
| Lint/UselessConstantScoping                 |    11 | -           |
| Lint/UselessDefaultValueArgument            |    24 | -           |
| Lint/UselessDefined                         |     7 | -           |
| Lint/UselessNumericOperation                |    13 | -           |
| Lint/UselessOr                              |   127 | -           |
| Lint/UselessRescue                          |    12 | -           |
| Lint/UselessRuby2Keywords                   |    23 | -           |

### Disabled by Default (4 cops, 95 tests)

| Cop                              | Tests | Status |
| -------------------------------- | ----: | ------ |
| Lint/ConstantResolution          |    18 | -      |
| Lint/HeredocMethodCallPosition   |    10 | -      |
| Lint/NumberConversion            |    36 | -      |
| Lint/ShadowingOuterLocalVariable |    31 | -      |

## Layout (7/100 implemented, 4,654 tests)

### Enabled by Default (81 cops, 4,067 tests)

| Cop                                              | Tests | Status      |
| ------------------------------------------------ | ----: | ----------- |
| Layout/AccessModifierIndentation                 |    43 | -           |
| Layout/ArgumentAlignment                         |    52 | -           |
| Layout/ArrayAlignment                            |    25 | -           |
| Layout/AssignmentIndentation                     |    10 | -           |
| Layout/BeginEndAlignment                         |     7 | -           |
| Layout/BlockAlignment                            |    78 | -           |
| Layout/BlockEndNewline                           |    18 | -           |
| Layout/CaseIndentation                           |    50 | -           |
| Layout/ClosingHeredocIndentation                 |    11 | -           |
| Layout/ClosingParenthesisIndentation             |    43 | -           |
| Layout/CommentIndentation                        |    28 | -           |
| Layout/ConditionPosition                         |    14 | -           |
| Layout/DefEndAlignment                           |    18 | -           |
| Layout/DotPosition                               |    39 | -           |
| Layout/ElseAlignment                             |    52 | -           |
| Layout/EmptyComment                              |    14 | -           |
| Layout/EmptyLineAfterGuardClause                 |    47 | -           |
| Layout/EmptyLineAfterMagicComment                |    21 | -           |
| Layout/EmptyLineBetweenDefs                      |    45 | -           |
| Layout/EmptyLines                                |     5 | -           |
| Layout/EmptyLinesAroundAccessModifier            |   176 | -           |
| Layout/EmptyLinesAroundArguments                 |    22 | -           |
| Layout/EmptyLinesAroundAttributeAccessor         |    20 | -           |
| Layout/EmptyLinesAroundBeginBody                 |    11 | -           |
| Layout/EmptyLinesAroundBlockBody                 |    20 | -           |
| Layout/EmptyLinesAroundClassBody                 |    46 | -           |
| Layout/EmptyLinesAroundExceptionHandlingKeywords |    24 | -           |
| Layout/EmptyLinesAroundMethodBody                |    14 | -           |
| Layout/EmptyLinesAroundModuleBody                |    38 | -           |
| Layout/EndAlignment                              |   207 | -           |
| Layout/EndOfLine                                 |    17 | -           |
| Layout/ExtraSpacing                              |    82 | -           |
| Layout/FirstArgumentIndentation                  |   139 | -           |
| Layout/FirstArrayElementIndentation              |    53 | -           |
| Layout/FirstHashElementIndentation               |    60 | -           |
| Layout/FirstParameterIndentation                 |    20 | -           |
| Layout/HashAlignment                             |   131 | -           |
| Layout/HeredocIndentation                        |   105 | -           |
| Layout/IndentationConsistency                    |    53 | -           |
| Layout/IndentationStyle                          |    25 | -           |
| Layout/IndentationWidth                          |   177 | -           |
| Layout/InitialIndentation                        |     8 | -           |
| Layout/LeadingCommentSpace                       |    27 | Implemented |
| Layout/LeadingEmptyLines                         |     9 | -           |
| Layout/LineLength                                |   192 | Implemented |
| Layout/MultilineArrayBraceLayout                 |    35 | -           |
| Layout/MultilineBlockLayout                      |    30 | -           |
| Layout/MultilineHashBraceLayout                  |    34 | -           |
| Layout/MultilineMethodCallBraceLayout            |    44 | -           |
| Layout/MultilineMethodCallIndentation            |   252 | Implemented |
| Layout/MultilineMethodDefinitionBraceLayout      |    26 | -           |
| Layout/MultilineOperationIndentation             |   101 | -           |
| Layout/ParameterAlignment                        |    19 | -           |
| Layout/RescueEnsureAlignment                     |    99 | -           |
| Layout/SpaceAfterColon                           |    12 | -           |
| Layout/SpaceAfterComma                           |     9 | Implemented |
| Layout/SpaceAfterMethodName                      |     8 | -           |
| Layout/SpaceAfterNot                             |     6 | -           |
| Layout/SpaceAfterSemicolon                       |     9 | -           |
| Layout/SpaceAroundBlockParameters                |    45 | -           |
| Layout/SpaceAroundEqualsInParameterDefault       |    11 | -           |
| Layout/SpaceAroundKeyword                        |   112 | -           |
| Layout/SpaceAroundMethodCallOperator             |    51 | -           |
| Layout/SpaceAroundOperators                      |    99 | -           |
| Layout/SpaceBeforeBlockBraces                    |    18 | -           |
| Layout/SpaceBeforeComma                          |     6 | -           |
| Layout/SpaceBeforeComment                        |     5 | -           |
| Layout/SpaceBeforeFirstArg                       |    12 | -           |
| Layout/SpaceBeforeSemicolon                      |     9 | -           |
| Layout/SpaceInLambdaLiteral                      |    15 | -           |
| Layout/SpaceInsideArrayLiteralBrackets           |    99 | -           |
| Layout/SpaceInsideArrayPercentLiteral            |   129 | -           |
| Layout/SpaceInsideBlockBraces                    |    43 | -           |
| Layout/SpaceInsideHashLiteralBraces              |    40 | -           |
| Layout/SpaceInsideParens                         |    28 | -           |
| Layout/SpaceInsidePercentLiteralDelimiters       |   262 | Implemented |
| Layout/SpaceInsideRangeLiteral                   |     7 | -           |
| Layout/SpaceInsideReferenceBrackets              |    47 | -           |
| Layout/SpaceInsideStringInterpolation            |    12 | -           |
| Layout/TrailingEmptyLines                        |    18 | Implemented |
| Layout/TrailingWhitespace                        |    19 | Implemented |

### Pending by Default (5 cops, 209 tests)

| Cop                                          | Tests | Status |
| -------------------------------------------- | ----: | ------ |
| Layout/EmptyLinesAfterModuleInclusion        |    59 | -      |
| Layout/LineContinuationLeadingSpace          |    32 | -      |
| Layout/LineContinuationSpacing               |    31 | -      |
| Layout/LineEndStringConcatenationIndentation |    59 | -      |
| Layout/SpaceBeforeBrackets                   |    28 | -      |

### Disabled by Default (14 cops, 378 tests)

| Cop                                       | Tests | Status |
| ----------------------------------------- | ----: | ------ |
| Layout/ClassStructure                     |    21 | -      |
| Layout/EmptyLineAfterMultilineCondition   |    22 | -      |
| Layout/FirstArrayElementLineBreak         |    14 | -      |
| Layout/FirstHashElementLineBreak          |    11 | -      |
| Layout/FirstMethodArgumentLineBreak       |    14 | -      |
| Layout/FirstMethodParameterLineBreak      |    11 | -      |
| Layout/HeredocArgumentClosingParenthesis  |    82 | -      |
| Layout/MultilineArrayLineBreaks           |     6 | -      |
| Layout/MultilineAssignmentLayout          |    34 | -      |
| Layout/MultilineHashKeyLineBreaks         |    10 | -      |
| Layout/MultilineMethodArgumentLineBreaks  |    18 | -      |
| Layout/MultilineMethodParameterLineBreaks |    14 | -      |
| Layout/RedundantLineBreak                 |   112 | -      |
| Layout/SingleLineBlockChain               |     9 | -      |

## Metrics (3/10 implemented, 272 tests)

### Enabled by Default (9 cops, 259 tests)

| Cop                          | Tests | Status      |
| ---------------------------- | ----: | ----------- |
| Metrics/AbcSize              |    25 | -           |
| Metrics/BlockLength          |    38 | Implemented |
| Metrics/BlockNesting         |    26 | -           |
| Metrics/ClassLength          |    34 | Implemented |
| Metrics/CyclomaticComplexity |    37 | -           |
| Metrics/MethodLength         |    31 | Implemented |
| Metrics/ModuleLength         |    21 | -           |
| Metrics/ParameterLists       |    16 | -           |
| Metrics/PerceivedComplexity  |    31 | -           |

### Pending by Default (1 cops, 13 tests)

| Cop                             | Tests | Status |
| ------------------------------- | ----: | ------ |
| Metrics/CollectionLiteralLength |    13 | -      |

## Naming (1/19 implemented, 2,217 tests)

### Enabled by Default (16 cops, 884 tests)

| Cop                                  | Tests | Status |
| ------------------------------------ | ----: | ------ |
| Naming/AccessorMethodName            |    23 | -      |
| Naming/AsciiIdentifiers              |    12 | -      |
| Naming/BinaryOperatorParameterName   |    15 | -      |
| Naming/BlockParameterName            |    13 | -      |
| Naming/ClassAndModuleCamelCase       |     5 | -      |
| Naming/ConstantName                  |    24 | -      |
| Naming/FileName                      |   120 | -      |
| Naming/HeredocDelimiterCase          |    26 | -      |
| Naming/HeredocDelimiterNaming        |    19 | -      |
| Naming/MemoizedInstanceVariableName  |    72 | -      |
| Naming/MethodName                    |   239 | -      |
| Naming/MethodParameterName           |    23 | -      |
| Naming/PredicatePrefix               |    24 | -      |
| Naming/RescuedExceptionsVariableName |    36 | -      |
| Naming/VariableName                  |   118 | -      |
| Naming/VariableNumber                |   115 | -      |

### Pending by Default (2 cops, 1,298 tests)

| Cop                    | Tests | Status      |
| ---------------------- | ----: | ----------- |
| Naming/BlockForwarding |    36 | -           |
| Naming/PredicateMethod |  1262 | Implemented |

### Disabled by Default (1 cops, 35 tests)

| Cop                      | Tests | Status |
| ------------------------ | ----: | ------ |
| Naming/InclusiveLanguage |    35 | -      |

## Gemspec (0/10 implemented, 193 tests)

### Enabled by Default (4 cops, 61 tests)

| Cop                             | Tests | Status |
| ------------------------------- | ----: | ------ |
| Gemspec/DuplicatedAssignment    |    17 | -      |
| Gemspec/OrderedDependencies     |    18 | -      |
| Gemspec/RequiredRubyVersion     |    21 | -      |
| Gemspec/RubyVersionGlobalsUsage |     5 | -      |

### Pending by Default (5 cops, 55 tests)

| Cop                                   | Tests | Status |
| ------------------------------------- | ----: | ------ |
| Gemspec/AddRuntimeDependency          |     5 | -      |
| Gemspec/AttributeAssignment           |     7 | -      |
| Gemspec/DeprecatedAttributeAssignment |    18 | -      |
| Gemspec/DevelopmentDependencies       |    13 | -      |
| Gemspec/RequireMFA                    |    12 | -      |

### Disabled by Default (1 cops, 77 tests)

| Cop                       | Tests | Status |
| ------------------------- | ----: | ------ |
| Gemspec/DependencyVersion |    77 | -      |

## Bundler (0/7 implemented, 101 tests)

### Enabled by Default (5 cops, 69 tests)

| Cop                            | Tests | Status |
| ------------------------------ | ----: | ------ |
| Bundler/DuplicatedGem          |    10 | -      |
| Bundler/DuplicatedGroup        |    21 | -      |
| Bundler/GemFilename            |    15 | -      |
| Bundler/InsecureProtocolSource |     6 | -      |
| Bundler/OrderedGems            |    17 | -      |

### Disabled by Default (2 cops, 32 tests)

| Cop                | Tests | Status |
| ------------------ | ----: | ------ |
| Bundler/GemComment |    26 | -      |
| Bundler/GemVersion |     6 | -      |

## Security (0/7 implemented, 102 tests)

### Enabled by Default (5 cops, 49 tests)

| Cop                  | Tests | Status |
| -------------------- | ----: | ------ |
| Security/Eval        |    15 | -      |
| Security/JSONLoad    |     7 | -      |
| Security/MarshalLoad |     5 | -      |
| Security/Open        |    16 | -      |
| Security/YAMLLoad    |     6 | -      |

### Pending by Default (2 cops, 53 tests)

| Cop                   | Tests | Status |
| --------------------- | ----: | ------ |
| Security/CompoundHash |    21 | -      |
| Security/IoMethods    |    32 | -      |

## Migration (0/1 implemented, 8 tests)

### Enabled by Default (1 cops, 8 tests)

| Cop                      | Tests | Status |
| ------------------------ | ----: | ------ |
| Migration/DepartmentName |     8 | -      |
