# All Cops State (606 total)

Full list of all RuboCop cops tracked by ruby-fast-cop, organized by department and default status.
184 of 606 implemented. See [README.md](README.md) for the implementation roadmap.

## Summary

| Department | Enabled | Pending | Disabled | Implemented |      Tests |
| ---------- | ------: | ------: | -------: | ----------: | ---------: |
| Style      |     175 |      91 |       32 |          86 |     14,567 |
| Lint       |     100 |      50 |        4 |          35 |      5,961 |
| Layout     |      81 |       5 |       14 |          50 |      4,654 |
| Metrics    |       9 |       1 |        0 |           5 |        272 |
| Naming     |      16 |       2 |        1 |           8 |      2,217 |
| Gemspec    |       4 |       5 |        1 |           0 |        193 |
| Bundler    |       5 |       0 |        2 |           0 |        101 |
| Security   |       5 |       2 |        0 |           0 |        102 |
| Migration  |       1 |       0 |        0 |           0 |          8 |
| **Total**  | **396** | **156** |   **54** |      **184** | **28,075** |

- **Enabled**: Runs by default on every codebase (highest priority to implement)
- **Pending**: Runs only with `NewCops: enable` in config
- **Disabled**: Runs only when explicitly enabled in config

## Style (86/298 implemented, 14,567 tests)

### Enabled by Default (175 cops, 9,202 tests)

| Cop                                    | Tests | Status      |
| -------------------------------------- | ----: | ----------- |
| Style/AccessModifierDeclarations       |   377 | Implemented |
| Style/AccessorGrouping                 |    26 | -           |
| Style/Alias                            |    26 | -           |
| Style/AndOr                            |    76 | Implemented |
| Style/ArrayIntersect                   |    81 | Implemented |
| Style/ArrayIntersectWithSingleElement  |     3 | -           |
| Style/ArrayJoin                        |     5 | -           |
| Style/Attr                             |    11 | -           |
| Style/BarePercentLiterals              |    36 | -           |
| Style/BeginBlock                       |     1 | -           |
| Style/BisectedAttrAccessor             |    14 | -           |
| Style/BlockComments                    |     5 | -           |
| Style/BlockDelimiters                  |   173 | Implemented |
| Style/CaseEquality                     |    25 | -           |
| Style/CaseLikeIf                       |    38 | Implemented |
| Style/CharacterLiteral                 |     5 | -           |
| Style/ClassAndModuleChildren           |    40 | -           |
| Style/ClassCheck                       |     4 | -           |
| Style/ClassEqualityComparison          |    22 | Implemented |
| Style/ClassMethods                     |     5 | -           |
| Style/ClassVars                        |     5 | -           |
| Style/ColonMethodCall                  |    10 | -           |
| Style/ColonMethodDefinition            |     3 | -           |
| Style/CombinableLoops                  |    20 | -           |
| Style/CommandLiteral                   |    35 | -           |
| Style/CommentAnnotation                |    31 | Implemented |
| Style/CommentedKeyword                 |    47 | Implemented |
| Style/ConditionalAssignment            |  1199 | Implemented |
| Style/DefWithParentheses               |     9 | -           |
| Style/Dir                              |     4 | -           |
| Style/Documentation                    |    55 | Implemented |
| Style/DoubleCopDisableDirective        |     3 | -           |
| Style/DoubleNegation                   |    47 | Implemented |
| Style/EachForSimpleLoop                |    20 | -           |
| Style/EachWithObject                   |    16 | -           |
| Style/EmptyBlockParameter              |     9 | -           |
| Style/EmptyCaseCondition               |    29 | Implemented |
| Style/EmptyElse                        |   124 | Implemented |
| Style/EmptyLambdaParameter             |     3 | -           |
| Style/EmptyLiteral                     |    49 | Implemented |
| Style/EmptyMethod                      |    32 | Implemented |
| Style/Encoding                         |    13 | -           |
| Style/EndBlock                         |     2 | -           |
| Style/EvalWithLocation                 |    27 | -           |
| Style/EvenOdd                          |    18 | -           |
| Style/ExpandPathArguments              |    16 | -           |
| Style/ExplicitBlockArgument            |    21 | -           |
| Style/ExponentialNotation              |    27 | -           |
| Style/FloatDivision                    |    31 | Implemented |
| Style/For                              |    32 | Implemented |
| Style/FormatString                     |    46 | -           |
| Style/FormatStringToken                |   366 | Implemented |
| Style/FrozenStringLiteralComment       |   107 | Implemented |
| Style/GlobalStdStream                  |     6 | -           |
| Style/GlobalVars                       |    74 | Implemented |
| Style/GuardClause                      |    91 | Implemented |
| Style/HashAsLastArrayItem              |    19 | -           |
| Style/HashEachMethods                  |    62 | Implemented |
| Style/HashLikeCase                     |     8 | -           |
| Style/HashSyntax                       |   189 | Implemented |
| Style/HashTransformKeys                |    40 | Implemented |
| Style/HashTransformValues              |    40 | Implemented |
| Style/IdenticalConditionalBranches     |    48 | Implemented |
| Style/IfInsideElse                     |    21 | -           |
| Style/IfUnlessModifier                 |   126 | Implemented |
| Style/IfUnlessModifierOfIfUnless       |     7 | -           |
| Style/IfWithSemicolon                  |    28 | Implemented |
| Style/InfiniteLoop                     |    28 | -           |
| Style/InverseMethods                   |   110 | Implemented |
| Style/KeywordParametersOrder           |    10 | -           |
| Style/Lambda                           |    38 | Implemented |
| Style/LambdaCall                       |    19 | -           |
| Style/LineEndConcatenation             |    19 | -           |
| Style/MethodCallWithoutArgsParentheses |    34 | Implemented |
| Style/MethodDefParentheses             |    49 | Implemented |
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
| Style/MultipleComparison               |    34 | Implemented |
| Style/MutableConstant                  |   354 | Implemented |
| Style/NegatedIf                        |    15 | Implemented |
| Style/NegatedUnless                    |    14 | Implemented |
| Style/NegatedWhile                     |    10 | Implemented |
| Style/NestedModifier                   |    13 | -           |
| Style/NestedParenthesizedCalls         |    12 | -           |
| Style/NestedTernaryOperator            |     7 | -           |
| Style/Next                             |    72 | Implemented |
| Style/NilComparison                    |     8 | -           |
| Style/NonNilCheck                      |    21 | -           |
| Style/Not                              |     9 | -           |
| Style/NumericLiteralPrefix             |    10 | -           |
| Style/NumericLiterals                  |    28 | Implemented |
| Style/NumericPredicate                 |    43 | Implemented |
| Style/OneLineConditional               |   108 | Implemented |
| Style/OptionalArguments                |    12 | -           |
| Style/OptionalBooleanParameter         |     8 | -           |
| Style/OrAssignment                     |    25 | -           |
| Style/ParallelAssignment               |    86 | -           |
| Style/ParenthesesAroundCondition       |    30 | Implemented |
| Style/PercentLiteralDelimiters         |    65 | Implemented |
| Style/PercentQLiterals                 |    21 | Implemented |
| Style/PerlBackrefs                     |    14 | -           |
| Style/PreferredHashMethods             |     9 | -           |
| Style/Proc                             |     6 | -           |
| Style/RaiseArgs                        |    35 | Implemented |
| Style/RandomWithOffset                 |    29 | -           |
| Style/RedundantAssignment              |    11 | -           |
| Style/RedundantBegin                   |    63 | Implemented |
| Style/RedundantCapitalW                |    13 | Implemented |
| Style/RedundantCondition               |   102 | Implemented |
| Style/RedundantConditional             |    11 | -           |
| Style/RedundantException               |    30 | Implemented |
| Style/RedundantFetchBlock              |    15 | -           |
| Style/RedundantFileExtensionInRequire  |     4 | -           |
| Style/RedundantFreeze                  |    62 | Implemented |
| Style/RedundantInterpolation           |    29 | Implemented |
| Style/RedundantParentheses             |   331 | Implemented |
| Style/RedundantPercentQ                |    25 | -           |
| Style/RedundantRegexpCharacterClass    |    47 | Implemented |
| Style/RedundantRegexpEscape            |   217 | Implemented |
| Style/RedundantReturn                  |    39 | Implemented |
| Style/RedundantSelf                    |    62 | Implemented |
| Style/RedundantSelfAssignment          |    14 | -           |
| Style/RedundantSort                    |    50 | Implemented |
| Style/RedundantSortBy                  |     8 | -           |
| Style/RegexpLiteral                    |    57 | Implemented |
| Style/RescueModifier                   |    21 | -           |
| Style/RescueStandardError              |    37 | Implemented |
| Style/SafeNavigation                   |   786 | Implemented |
| Style/Sample                           |    82 | Implemented |
| Style/SelfAssignment                   |   105 | Implemented |
| Style/Semicolon                        |    33 | Implemented |
| Style/SignalException                  |    27 | -           |
| Style/SingleArgumentDig                |    15 | -           |
| Style/SingleLineMethods                |    16 | -           |
| Style/SlicingWithRange                 |    28 | -           |
| Style/SoleNestedConditional            |    73 | Implemented |
| Style/SpecialGlobalVars                |    31 | Implemented |
| Style/StabbyLambdaParentheses          |     9 | -           |
| Style/StderrPuts                       |     5 | -           |
| Style/StringConcatenation              |    30 | Implemented |
| Style/StringLiterals                   |    58 | Implemented |
| Style/StringLiteralsInInterpolation    |    13 | -           |
| Style/Strip                            |     6 | -           |
| Style/StructInheritance                |    12 | -           |
| Style/SymbolArray                      |    33 | Implemented |
| Style/SymbolLiteral                    |     4 | -           |
| Style/SymbolProc                       |    83 | Implemented |
| Style/TernaryParentheses               |    98 | Implemented |
| Style/TrailingBodyOnClass              |     7 | -           |
| Style/TrailingBodyOnMethodDefinition   |    12 | -           |
| Style/TrailingBodyOnModule             |     7 | -           |
| Style/TrailingCommaInArguments         |   178 | Implemented |
| Style/TrailingCommaInArrayLiteral      |    48 | Implemented |
| Style/TrailingCommaInHashLiteral       |    41 | Implemented |
| Style/TrailingMethodEndStatement       |    10 | -           |
| Style/TrailingUnderscoreVariable       |    58 | Implemented |
| Style/TrivialAccessors                 |    38 | Implemented |
| Style/UnlessElse                       |     5 | -           |
| Style/UnpackFirst                      |    11 | -           |
| Style/VariableInterpolation            |     9 | Implemented |
| Style/WhenThen                         |     4 | -           |
| Style/WhileUntilDo                     |     6 | -           |
| Style/WhileUntilModifier               |    34 | Implemented |
| Style/WordArray                        |    59 | Implemented |
| Style/YodaCondition                    |    73 | Implemented |
| Style/ZeroLengthPredicate              |    68 | Implemented |

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

## Lint (35/154 implemented, 5,961 tests)

### Enabled by Default (100 cops, 3,859 tests)

| Cop                                      | Tests | Status      |
| ---------------------------------------- | ----: | ----------- |
| Lint/AmbiguousBlockAssociation           |    36 | Implemented |
| Lint/AmbiguousOperator                   |    17 | -           |
| Lint/AmbiguousRegexpLiteral              |    30 | Implemented |
| Lint/AssignmentInCondition               |    69 | Implemented |
| Lint/BigDecimalNew                       |     3 | -           |
| Lint/BinaryOperatorWithIdenticalOperands |    23 | -           |
| Lint/BooleanSymbol                       |    10 | -           |
| Lint/CircularArgumentReference           |    13 | -           |
| Lint/ConstantDefinitionInBlock           |    27 | -           |
| Lint/Debugger                            |    97 | Implemented |
| Lint/DeprecatedClassMethods              |    31 | Implemented |
| Lint/DeprecatedOpenSSLConstant           |    24 | -           |
| Lint/DisjunctiveAssignmentInConstructor  |     7 | -           |
| Lint/DuplicateCaseCondition              |     9 | -           |
| Lint/DuplicateElsifCondition             |     5 | -           |
| Lint/DuplicateHashKey                    |    33 | Implemented |
| Lint/DuplicateMethods                    |   329 | Implemented |
| Lint/DuplicateRequire                    |    10 | -           |
| Lint/DuplicateRescueException            |     6 | -           |
| Lint/EachWithObjectArgument              |     7 | -           |
| Lint/ElseLayout                          |    12 | -           |
| Lint/EmptyConditionalBody                |    42 | Implemented |
| Lint/EmptyEnsure                         |     2 | -           |
| Lint/EmptyExpression                     |    12 | -           |
| Lint/EmptyFile                           |     2 | -           |
| Lint/EmptyInterpolation                  |    12 | Implemented |
| Lint/EmptyWhen                           |    16 | -           |
| Lint/EnsureReturn                        |     5 | -           |
| Lint/ErbNewArguments                     |    10 | -           |
| Lint/FlipFlop                            |     2 | -           |
| Lint/FloatComparison                     |    17 | -           |
| Lint/FloatOutOfRange                     |     5 | -           |
| Lint/FormatParameterMismatch             |    75 | Implemented |
| Lint/HashCompareByIdentity               |     4 | -           |
| Lint/IdentityComparison                  |    12 | -           |
| Lint/ImplicitStringConcatenation         |    12 | -           |
| Lint/IneffectiveAccessModifier           |     8 | -           |
| Lint/InheritException                    |    13 | -           |
| Lint/InterpolationCheck                  |    15 | -           |
| Lint/LiteralAsCondition                  |   229 | Implemented |
| Lint/LiteralInInterpolation              |   378 | Implemented |
| Lint/Loop                                |     4 | -           |
| Lint/MissingCopEnableDirective           |    11 | -           |
| Lint/MissingSuper                        |    22 | -           |
| Lint/MixedRegexpCaptureTypes             |    12 | -           |
| Lint/MultipleComparison                  |    20 | -           |
| Lint/NestedMethodDefinition              |    38 | Implemented |
| Lint/NestedPercentLiteral                |    11 | Implemented |
| Lint/NextWithoutAccumulator              |    18 | -           |
| Lint/NonDeterministicRequireOrder        |    28 | -           |
| Lint/NonLocalExitFromIterator            |    14 | -           |
| Lint/OrderedMagicComments                |    10 | -           |
| Lint/OutOfRangeRegexpRef                 |   122 | Implemented |
| Lint/ParenthesesAsGroupedExpression      |    26 | -           |
| Lint/PercentStringArray                  |    22 | Implemented |
| Lint/PercentSymbolArray                  |    12 | Implemented |
| Lint/RaiseException                      |    15 | -           |
| Lint/RandOne                             |    16 | -           |
| Lint/RedundantCopDisableDirective        |    44 | -           |
| Lint/RedundantCopEnableDirective         |    23 | Implemented |
| Lint/RedundantRequireStatement           |    15 | -           |
| Lint/RedundantSafeNavigation             |    72 | Implemented |
| Lint/RedundantSplatExpansion             |    59 | Implemented |
| Lint/RedundantStringCoercion             |    18 | Implemented |
| Lint/RedundantWithIndex                  |    17 | -           |
| Lint/RedundantWithObject                 |    14 | -           |
| Lint/RegexpAsCondition                   |     5 | -           |
| Lint/RequireParentheses                  |    16 | -           |
| Lint/RescueException                     |    11 | -           |
| Lint/RescueType                          |    52 | Implemented |
| Lint/ReturnInVoidContext                 |    18 | -           |
| Lint/SafeNavigationChain                 |    63 | Implemented |
| Lint/SafeNavigationConsistency           |    43 | Implemented |
| Lint/SafeNavigationWithEmpty             |     3 | -           |
| Lint/ScriptPermission                    |     6 | -           |
| Lint/SelfAssignment                      |    58 | Implemented |
| Lint/SendWithMixinArgument               |    14 | -           |
| Lint/ShadowedArgument                    |    54 | Implemented |
| Lint/ShadowedException                   |    38 | Implemented |
| Lint/StructNewOverride                   |    10 | -           |
| Lint/SuppressedException                 |    24 | -           |
| Lint/Syntax                              |     0 | -           |
| Lint/ToJSON                              |     2 | -           |
| Lint/TopLevelReturnWithArgument          |    10 | -           |
| Lint/TrailingCommaInAttributeDeclaration |     2 | -           |
| Lint/UnderscorePrefixedVariableName      |    19 | -           |
| Lint/UnifiedInteger                      |    15 | -           |
| Lint/UnreachableCode                     |   266 | Implemented |
| Lint/UnreachableLoop                     |    28 | Implemented |
| Lint/UnusedBlockArgument                 |    30 | Implemented |
| Lint/UnusedMethodArgument                |    41 | Implemented |
| Lint/UriEscapeUnescape                   |     9 | -           |
| Lint/UriRegexp                           |    10 | -           |
| Lint/UselessAccessModifier               |   198 | Implemented |
| Lint/UselessAssignment                   |   149 | Implemented |
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

## Layout (50/100 implemented, 4,654 tests)

### Enabled by Default (81 cops, 4,067 tests)

| Cop                                              | Tests | Status      |
| ------------------------------------------------ | ----: | ----------- |
| Layout/AccessModifierIndentation                 |    43 | Implemented |
| Layout/ArgumentAlignment                         |    52 | -           |
| Layout/ArrayAlignment                            |    25 | -           |
| Layout/AssignmentIndentation                     |    10 | -           |
| Layout/BeginEndAlignment                         |     7 | Implemented |
| Layout/BlockAlignment                            |    78 | Implemented |
| Layout/BlockEndNewline                           |    18 | -           |
| Layout/CaseIndentation                           |    50 | Implemented |
| Layout/ClosingHeredocIndentation                 |    11 | -           |
| Layout/ClosingParenthesisIndentation             |    43 | Implemented |
| Layout/CommentIndentation                        |    28 | -           |
| Layout/ConditionPosition                         |    14 | -           |
| Layout/DefEndAlignment                           |    18 | Implemented |
| Layout/DotPosition                               |    39 | -           |
| Layout/ElseAlignment                             |    52 | Implemented |
| Layout/EmptyComment                              |    14 | -           |
| Layout/EmptyLineAfterGuardClause                 |    47 | Implemented |
| Layout/EmptyLineAfterMagicComment                |    21 | -           |
| Layout/EmptyLineBetweenDefs                      |    45 | Implemented |
| Layout/EmptyLines                                |     5 | -           |
| Layout/EmptyLinesAroundAccessModifier            |   176 | Implemented |
| Layout/EmptyLinesAroundArguments                 |    22 | -           |
| Layout/EmptyLinesAroundAttributeAccessor         |    20 | -           |
| Layout/EmptyLinesAroundBeginBody                 |    11 | Implemented |
| Layout/EmptyLinesAroundBlockBody                 |    20 | Implemented |
| Layout/EmptyLinesAroundClassBody                 |    46 | Implemented |
| Layout/EmptyLinesAroundExceptionHandlingKeywords |    24 | Implemented |
| Layout/EmptyLinesAroundMethodBody                |    14 | Implemented |
| Layout/EmptyLinesAroundModuleBody                |    38 | Implemented |
| Layout/EndAlignment                              |   207 | Implemented |
| Layout/EndOfLine                                 |    17 | -           |
| Layout/ExtraSpacing                              |    82 | Implemented |
| Layout/FirstArgumentIndentation                  |   139 | Implemented |
| Layout/FirstArrayElementIndentation              |    53 | Implemented |
| Layout/FirstHashElementIndentation               |    60 | Implemented |
| Layout/FirstParameterIndentation                 |    20 | -           |
| Layout/HashAlignment                             |   131 | Implemented |
| Layout/HeredocIndentation                        |   105 | Implemented |
| Layout/IndentationConsistency                    |    53 | Implemented |
| Layout/IndentationStyle                          |    25 | -           |
| Layout/IndentationWidth                          |   177 | Implemented |
| Layout/InitialIndentation                        |     8 | -           |
| Layout/LeadingCommentSpace                       |    27 | Implemented |
| Layout/LeadingEmptyLines                         |     9 | -           |
| Layout/LineLength                                |   192 | Implemented |
| Layout/MultilineArrayBraceLayout                 |    35 | Implemented |
| Layout/MultilineBlockLayout                      |    30 | -           |
| Layout/MultilineHashBraceLayout                  |    34 | Implemented |
| Layout/MultilineMethodCallBraceLayout            |    44 | Implemented |
| Layout/MultilineMethodCallIndentation            |   252 | Implemented |
| Layout/MultilineMethodDefinitionBraceLayout      |    26 | -           |
| Layout/MultilineOperationIndentation             |   101 | Implemented |
| Layout/ParameterAlignment                        |    19 | -           |
| Layout/RescueEnsureAlignment                     |    99 | Implemented |
| Layout/SpaceAfterColon                           |    12 | -           |
| Layout/SpaceAfterComma                           |     9 | Implemented |
| Layout/SpaceAfterMethodName                      |     8 | -           |
| Layout/SpaceAfterNot                             |     6 | -           |
| Layout/SpaceAfterSemicolon                       |     9 | -           |
| Layout/SpaceAroundBlockParameters                |    45 | Implemented |
| Layout/SpaceAroundEqualsInParameterDefault       |    11 | Implemented |
| Layout/SpaceAroundKeyword                        |   112 | Implemented |
| Layout/SpaceAroundMethodCallOperator             |    51 | Implemented |
| Layout/SpaceAroundOperators                      |    99 | Implemented |
| Layout/SpaceBeforeBlockBraces                    |    18 | -           |
| Layout/SpaceBeforeComma                          |     6 | -           |
| Layout/SpaceBeforeComment                        |     5 | -           |
| Layout/SpaceBeforeFirstArg                       |    12 | Implemented |
| Layout/SpaceBeforeSemicolon                      |     9 | -           |
| Layout/SpaceInLambdaLiteral                      |    15 | -           |
| Layout/SpaceInsideArrayLiteralBrackets           |    99 | Implemented |
| Layout/SpaceInsideArrayPercentLiteral            |   129 | Implemented |
| Layout/SpaceInsideBlockBraces                    |    43 | Implemented |
| Layout/SpaceInsideHashLiteralBraces              |    40 | Implemented |
| Layout/SpaceInsideParens                         |    28 | Implemented |
| Layout/SpaceInsidePercentLiteralDelimiters       |   262 | Implemented |
| Layout/SpaceInsideRangeLiteral                   |     7 | -           |
| Layout/SpaceInsideReferenceBrackets              |    47 | Implemented |
| Layout/SpaceInsideStringInterpolation            |    12 | Implemented |
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

## Metrics (5/10 implemented, 272 tests)

### Enabled by Default (9 cops, 259 tests)

| Cop                          | Tests | Status      |
| ---------------------------- | ----: | ----------- |
| Metrics/AbcSize              |    25 | -           |
| Metrics/BlockLength          |    38 | Implemented |
| Metrics/BlockNesting         |    26 | -           |
| Metrics/ClassLength          |    34 | Implemented |
| Metrics/CyclomaticComplexity |    37 | Implemented |
| Metrics/MethodLength         |    31 | Implemented |
| Metrics/ModuleLength         |    21 | -           |
| Metrics/ParameterLists       |    16 | -           |
| Metrics/PerceivedComplexity  |    31 | Implemented |

### Pending by Default (1 cops, 13 tests)

| Cop                             | Tests | Status |
| ------------------------------- | ----: | ------ |
| Metrics/CollectionLiteralLength |    13 | -      |

## Naming (8/19 implemented, 2,217 tests)

### Enabled by Default (16 cops, 884 tests)

| Cop                                  | Tests | Status      |
| ------------------------------------ | ----: | ----------- |
| Naming/AccessorMethodName            |    23 | -           |
| Naming/AsciiIdentifiers              |    12 | -           |
| Naming/BinaryOperatorParameterName   |    15 | -           |
| Naming/BlockParameterName            |    13 | Implemented |
| Naming/ClassAndModuleCamelCase       |     5 | -           |
| Naming/ConstantName                  |    24 | -           |
| Naming/FileName                      |   120 | Implemented |
| Naming/HeredocDelimiterCase          |    26 | -           |
| Naming/HeredocDelimiterNaming        |    19 | -           |
| Naming/MemoizedInstanceVariableName  |    72 | Implemented |
| Naming/MethodName                    |   239 | Implemented |
| Naming/MethodParameterName           |    23 | Implemented |
| Naming/PredicatePrefix               |    24 | -           |
| Naming/RescuedExceptionsVariableName |    36 | -           |
| Naming/VariableName                  |   118 | Implemented |
| Naming/VariableNumber                |   115 | Implemented |

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

## Implementation Clusters (Unimplemented, Enabled by Default)

220 cops / 3189 tests, grouped into 51 clusters for future batches.

- **Mixin clusters** share a RuboCop mixin — port `RuboCop::Cop::<Mixin>` once into `src/helpers/`, reuse across all cops in the cluster.
- **Topic clusters** have no unique shared mixin — grouped by name-family / responsibility, implement individually.
- Three mixins (`RangeHelp`, `Alignment`, `ConfigurableEnforcedStyle`) are too generic to cluster on — they show up as `_(+ X)_` annotations.
- Difficulty is rough (from max Ruby LOC in cluster): Easy <50, Med <120, Hard ≥120.
- Tests & status live in the tables above — this section is just the cop → cluster map.

| # | Cluster | Kind | Cops | Tests | Diff |
|--:|---------|------|-----:|------:|------|
| 1 | Other | topic | 21 | 256 | Med |
| 2 | Method def/params | topic | 15 | 244 | Med |
| 3 | Block/lambda/proc | topic | 15 | 195 | Med |
| 4 | Redundant/Useless | topic | 13 | 182 | Med |
| 5 | Regexp/numeric | topic | 10 | 165 | Med |
| 6 | Rescue/ensure/exception | topic | 9 | 165 | Med |
| 7 | Class/module/attr | topic | 12 | 155 | Hard |
| 8 | Alignment/spacing | topic | 9 | 147 | Med |
| 9 | Return/ctrl flow | topic | 10 | 116 | Med |
| 10 | RescueNode | mixin | 3 | 113 | Hard |
| 11 | Comparison/equality | topic | 7 | 109 | Med |
| 12 | Require/load/file | topic | 9 | 109 | Med |
| 13 | AllowedMethods | mixin | 5 | 91 | Med |
| 14 | Empty constructs | topic | 9 | 90 | Med |
| 15 | String/interpolation | topic | 3 | 77 | Med |
| 16 | Hash/array/dig | topic | 6 | 73 | Med |
| 17 | Multiline expr/body | topic | 4 | 71 | Med |
| 18 | Cop directive comments | topic | 3 | 58 | Hard |
| 19 | Heredoc | mixin | 3 | 56 | Med |
| 20 | Duplicate detection | topic | 5 | 55 | Med |
| 21 | Deprecated/legacy APIs | topic | 4 | 52 | Med |
| 22 | Eval/send/URI | topic | 2 | 42 | Hard |
| 23 | Percent literal | topic | 1 | 36 | Easy |
| 24 | Naming | topic | 2 | 36 | Easy |
| 25 | OrderedGemNode | mixin | 2 | 35 | Easy |
| 26 | CommentsHelp | mixin | 2 | 33 | Med |
| 27 | Negated/Not | topic | 2 | 30 | Med |
| 28 | MultilineLiteralBraceLayout | mixin | 1 | 26 | Easy |
| 29 | TrailingBody | mixin | 3 | 26 | Easy |
| 30 | VisibilityHelp | mixin | 1 | 26 | Hard |
| 31 | MethodComplexity | mixin | 1 | 25 | Easy |
| 32 | FrozenStringLiteral | mixin | 2 | 25 | Med |
| 33 | GemspecHelp | mixin | 2 | 22 | Med |
| 34 | CodeLength | mixin | 1 | 21 | Easy |
| 35 | Gemspec/Bundler | topic | 2 | 21 | Med |
| 36 | MultilineElementIndentation | mixin | 1 | 20 | Easy |
| 37 | Nested constructs | topic | 2 | 20 | Med |
| 38 | StatementModifier | mixin | 2 | 17 | Easy |
| 39 | Security | topic | 1 | 16 | Easy |
| 40 | SpaceBeforePunctuation | mixin | 2 | 15 | Easy |
| 41 | DigHelp | mixin | 1 | 15 | Easy |
| 42 | StringLiteralsHelp | mixin | 1 | 13 | Easy |
| 43 | Magic comments/encoding | topic | 1 | 13 | Easy |
| 44 | EmptyParameter | mixin | 2 | 12 | Easy |
| 45 | Trailing body/comma | topic | 2 | 12 | Easy |
| 46 | OnNormalIfUnless | mixin | 1 | 11 | Easy |
| 47 | CheckAssignment | mixin | 1 | 10 | Easy |
| 48 | IntegerNode | mixin | 1 | 10 | Med |
| 49 | SpaceAfterPunctuation | mixin | 1 | 9 | Easy |
| 50 | MinBranchesCount | mixin | 1 | 8 | Easy |
| 51 | StringHelp | mixin | 1 | 5 | Easy |

### 1. Other — 21 cops, 256 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/CommandLiteral` _(+ ConfigurableEnforcedStyle)_
- `Lint/ParenthesesAsGroupedExpression` _(+ RangeHelp)_
- `Style/Alias` _(+ ConfigurableEnforcedStyle)_
- `Style/OrAssignment`
- `Style/CombinableLoops`
- `Lint/AmbiguousOperator`
- `Style/PerlBackrefs`
- `Style/UnpackFirst`
- `Lint/BooleanSymbol`
- `Lint/UriEscapeUnescape`
- `Style/DefWithParentheses` _(+ RangeHelp)_
- `Lint/IneffectiveAccessModifier`
- `Style/MissingRespondToMissing`
- `Migration/DepartmentName` _(+ RangeHelp)_
- `Lint/DisjunctiveAssignmentInConstructor`
- `Style/GlobalStdStream`
- `Style/Strip` _(+ RangeHelp)_
- `Style/StderrPuts` _(+ RangeHelp)_
- `Style/SymbolLiteral`
- `Lint/ToJSON`
- `Lint/Syntax`

### 2. Method def/params — 15 cops, 244 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Layout/ArgumentAlignment` _(+ Alignment)_
- `Naming/AccessorMethodName`
- `Lint/UnderscorePrefixedVariableName`
- `Layout/ParameterAlignment` _(+ Alignment)_
- `Lint/ReturnInVoidContext`
- `Metrics/ParameterLists`
- `Style/SingleLineMethods` _(+ Alignment)_
- `Naming/BinaryOperatorParameterName`
- `Lint/CircularArgumentReference`
- `Style/OptionalArguments`
- `Lint/TopLevelReturnWithArgument`
- `Style/KeywordParametersOrder` _(+ RangeHelp)_
- `Style/ColonMethodCall`
- `Layout/SpaceAfterMethodName` _(+ RangeHelp)_
- `Style/ColonMethodDefinition`

### 3. Block/lambda/proc — 15 cops, 195 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Metrics/BlockNesting`
- `Style/ExplicitBlockArgument` _(+ RangeHelp)_
- `Style/LambdaCall` _(+ ConfigurableEnforcedStyle)_
- `Lint/NextWithoutAccumulator`
- `Layout/SpaceBeforeBlockBraces` _(+ ConfigurableEnforcedStyle, RangeHelp)_
- `Layout/BlockEndNewline` _(+ Alignment)_
- `Style/EachWithObject` _(+ RangeHelp)_
- `Layout/SpaceInLambdaLiteral` _(+ ConfigurableEnforcedStyle, RangeHelp)_
- `Lint/NonLocalExitFromIterator`
- `Style/StabbyLambdaParentheses` _(+ ConfigurableEnforcedStyle)_
- `Lint/EachWithObjectArgument`
- `Style/Proc`
- `Style/BlockComments` _(+ RangeHelp)_
- `Style/EndBlock`
- `Style/BeginBlock`

### 4. Redundant/Useless — 13 cops, 182 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Lint/UselessTimes` _(+ RangeHelp)_
- `Style/RedundantPercentQ`
- `Lint/UselessSetterCall`
- `Lint/RedundantWithIndex` _(+ RangeHelp)_
- `Lint/UselessMethodDefinition`
- `Lint/RedundantRequireStatement` _(+ RangeHelp)_
- `Lint/RedundantWithObject` _(+ RangeHelp)_
- `Style/RedundantSelfAssignment` _(+ RangeHelp)_
- `Style/RedundantConditional` _(+ Alignment)_
- `Style/RedundantAssignment`
- `Style/RedundantSortBy` _(+ RangeHelp)_
- `Style/RedundantFileExtensionInRequire` _(+ RangeHelp)_
- `Lint/UselessElseWithoutRescue`

### 5. Regexp/numeric — 10 cops, 165 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/RandomWithOffset`
- `Style/SlicingWithRange`
- `Style/ExponentialNotation` _(+ ConfigurableEnforcedStyle)_
- `Style/EvenOdd`
- `Lint/RandOne`
- `Lint/InterpolationCheck`
- `Lint/MixedRegexpCaptureTypes`
- `Lint/UriRegexp`
- `Lint/FloatOutOfRange`
- `Lint/RegexpAsCondition`

### 6. Rescue/ensure/exception — 9 cops, 165 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Naming/RescuedExceptionsVariableName`
- `Style/SignalException` _(+ ConfigurableEnforcedStyle)_
- `Lint/SuppressedException`
- `Lint/MissingSuper`
- `Lint/RaiseException`
- `Lint/InheritException` _(+ ConfigurableEnforcedStyle)_
- `Style/StructInheritance` _(+ RangeHelp)_
- `Lint/RescueException`
- `Lint/EnsureReturn`

### 7. Class/module/attr — 12 cops, 155 tests (Hard)
Topic family, no unique shared mixin — implement individually.

- `Style/ClassAndModuleChildren` _(+ Alignment, ConfigurableEnforcedStyle, RangeHelp)_
- `Style/MixinUsage`
- `Style/MixinGrouping` _(+ ConfigurableEnforcedStyle)_
- `Lint/SendWithMixinArgument` _(+ RangeHelp)_
- `Style/BisectedAttrAccessor` _(+ RangeHelp)_
- `Style/Attr` _(+ RangeHelp)_
- `Style/ModuleFunction` _(+ ConfigurableEnforcedStyle)_
- `Lint/StructNewOverride`
- `Style/ClassVars`
- `Style/ClassMethods`
- `Naming/ClassAndModuleCamelCase`
- `Style/ClassCheck` _(+ ConfigurableEnforcedStyle)_

### 8. Alignment/spacing — 9 cops, 147 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Layout/DotPosition` _(+ ConfigurableEnforcedStyle, RangeHelp)_
- `Layout/CommentIndentation` _(+ Alignment)_
- `Layout/IndentationStyle` _(+ Alignment, ConfigurableEnforcedStyle, RangeHelp)_
- `Layout/EndOfLine` _(+ ConfigurableEnforcedStyle, RangeHelp)_
- `Layout/SpaceAfterColon`
- `Layout/InitialIndentation` _(+ RangeHelp)_
- `Layout/SpaceInsideRangeLiteral`
- `Layout/SpaceAfterNot` _(+ RangeHelp)_
- `Layout/SpaceBeforeComment`

### 9. Return/ctrl flow — 10 cops, 116 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/InfiniteLoop` _(+ Alignment)_
- `Style/IfInsideElse` _(+ RangeHelp)_
- `Style/EachForSimpleLoop`
- `Layout/ConditionPosition` _(+ RangeHelp)_
- `Lint/ElseLayout` _(+ Alignment, RangeHelp)_
- `Style/WhileUntilDo`
- `Style/UnlessElse`
- `Lint/Loop`
- `Style/WhenThen`
- `Lint/FlipFlop`

### 10. `RescueNode` mixin — 3 cops, 113 tests (Hard)
Port `RuboCop::Cop::RescueNode` once → reuse across all cops in this cluster.

- `Style/ParallelAssignment`
- `Style/RescueModifier` _(+ Alignment, RangeHelp)_
- `Lint/DuplicateRescueException`

### 11. Comparison/equality — 7 cops, 109 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/CaseEquality`
- `Lint/BinaryOperatorWithIdenticalOperands`
- `Lint/MultipleComparison`
- `Lint/FloatComparison`
- `Lint/IdentityComparison`
- `Style/NilComparison` _(+ ConfigurableEnforcedStyle)_
- `Lint/HashCompareByIdentity`

### 12. Require/load/file — 9 cops, 109 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Lint/NonDeterministicRequireOrder`
- `Gemspec/RequiredRubyVersion`
- `Lint/RequireParentheses` _(+ RangeHelp)_
- `Style/ExpandPathArguments` _(+ RangeHelp)_
- `Security/JSONLoad`
- `Lint/ScriptPermission`
- `Security/YAMLLoad`
- `Security/MarshalLoad`
- `Style/Dir`

### 13. `AllowedMethods` mixin — 5 cops, 91 tests (Med)
Port `RuboCop::Cop::AllowedMethods` once → reuse across all cops in this cluster.

- `Lint/ConstantDefinitionInBlock`
- `Naming/PredicatePrefix`
- `Layout/EmptyLinesAroundAttributeAccessor` _(+ RangeHelp)_
- `Style/NestedParenthesizedCalls` _(+ RangeHelp)_
- `Style/OptionalBooleanParameter`

### 14. Empty constructs — 9 cops, 90 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Layout/EmptyLinesAroundArguments` _(+ RangeHelp)_
- `Layout/EmptyLineAfterMagicComment` _(+ RangeHelp)_
- `Layout/EmptyComment` _(+ RangeHelp)_
- `Lint/EmptyExpression`
- `Layout/LeadingEmptyLines`
- `Layout/EmptyLines` _(+ RangeHelp)_
- `Lint/SafeNavigationWithEmpty`
- `Lint/EmptyEnsure`
- `Lint/EmptyFile`

### 15. String/interpolation — 3 cops, 77 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/FormatString` _(+ ConfigurableEnforcedStyle)_
- `Style/LineEndConcatenation` _(+ RangeHelp)_
- `Lint/ImplicitStringConcatenation`

### 16. Hash/array/dig — 6 cops, 73 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Layout/ArrayAlignment` _(+ Alignment)_
- `Style/HashAsLastArrayItem` _(+ RangeHelp, ConfigurableEnforcedStyle)_
- `Style/MinMax`
- `Style/PreferredHashMethods` _(+ ConfigurableEnforcedStyle)_
- `Style/ArrayJoin`
- `Style/ArrayIntersectWithSingleElement`

### 17. Multiline expr/body — 4 cops, 71 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Layout/MultilineBlockLayout` _(+ RangeHelp)_
- `Style/MultilineMemoization` _(+ Alignment, ConfigurableEnforcedStyle)_
- `Style/MultilineWhenThen` _(+ RangeHelp)_
- `Style/MultilineBlockChain` _(+ RangeHelp)_

### 18. Cop directive comments — 3 cops, 58 tests (Hard)
Topic family, no unique shared mixin — implement individually.

- `Lint/RedundantCopDisableDirective` _(+ RangeHelp)_
- `Lint/MissingCopEnableDirective` _(+ RangeHelp)_
- `Style/DoubleCopDisableDirective`

### 19. `Heredoc` mixin — 3 cops, 56 tests (Med)
Port `RuboCop::Cop::Heredoc` once → reuse across all cops in this cluster.

- `Naming/HeredocDelimiterCase` _(+ ConfigurableEnforcedStyle)_
- `Naming/HeredocDelimiterNaming`
- `Layout/ClosingHeredocIndentation`

### 20. Duplicate detection — 5 cops, 55 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Bundler/DuplicatedGroup` _(+ RangeHelp)_
- `Lint/DuplicateRequire` _(+ RangeHelp)_
- `Bundler/DuplicatedGem` _(+ RangeHelp)_
- `Lint/DuplicateCaseCondition`
- `Lint/DuplicateElsifCondition`

### 21. Deprecated/legacy APIs — 4 cops, 52 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Lint/DeprecatedOpenSSLConstant` _(+ RangeHelp)_
- `Lint/UnifiedInteger`
- `Lint/ErbNewArguments` _(+ RangeHelp)_
- `Lint/BigDecimalNew`

### 22. Eval/send/URI — 2 cops, 42 tests (Hard)
Topic family, no unique shared mixin — implement individually.

- `Style/EvalWithLocation`
- `Security/Eval`

### 23. Percent literal — 1 cops, 36 tests (Easy)
Topic family, no unique shared mixin — implement individually.

- `Style/BarePercentLiterals` _(+ ConfigurableEnforcedStyle)_

### 24. Naming — 2 cops, 36 tests (Easy)
Topic family, no unique shared mixin — implement individually.

- `Naming/ConstantName`
- `Naming/AsciiIdentifiers` _(+ RangeHelp)_

### 25. `OrderedGemNode` mixin — 2 cops, 35 tests (Easy)
Port `RuboCop::Cop::OrderedGemNode` once → reuse across all cops in this cluster.

- `Gemspec/OrderedDependencies`
- `Bundler/OrderedGems`

### 26. `CommentsHelp` mixin — 2 cops, 33 tests (Med)
Port `RuboCop::Cop::CommentsHelp` once → reuse across all cops in this cluster.

- `Style/MultilineTernaryOperator`
- `Lint/EmptyWhen`

### 27. Negated/Not — 2 cops, 30 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/NonNilCheck`
- `Style/Not` _(+ RangeHelp)_

### 28. `MultilineLiteralBraceLayout` mixin — 1 cops, 26 tests (Easy)
Port `RuboCop::Cop::MultilineLiteralBraceLayout` once → reuse across all cops in this cluster.

- `Layout/MultilineMethodDefinitionBraceLayout`

### 29. `TrailingBody` mixin — 3 cops, 26 tests (Easy)
Port `RuboCop::Cop::TrailingBody` once → reuse across all cops in this cluster.

- `Style/TrailingBodyOnMethodDefinition` _(+ Alignment)_
- `Style/TrailingBodyOnModule` _(+ Alignment)_
- `Style/TrailingBodyOnClass` _(+ Alignment)_

### 30. `VisibilityHelp` mixin — 1 cops, 26 tests (Hard)
Port `RuboCop::Cop::VisibilityHelp` once → reuse across all cops in this cluster.

- `Style/AccessorGrouping` _(+ ConfigurableEnforcedStyle, RangeHelp)_

### 31. `MethodComplexity` mixin — 1 cops, 25 tests (Easy)
Port `RuboCop::Cop::MethodComplexity` once → reuse across all cops in this cluster.

- `Metrics/AbcSize`

### 32. `FrozenStringLiteral` mixin — 2 cops, 25 tests (Med)
Port `RuboCop::Cop::FrozenStringLiteral` once → reuse across all cops in this cluster.

- `Style/RedundantFetchBlock` _(+ RangeHelp)_
- `Lint/OrderedMagicComments`

### 33. `GemspecHelp` mixin — 2 cops, 22 tests (Med)
Port `RuboCop::Cop::GemspecHelp` once → reuse across all cops in this cluster.

- `Gemspec/DuplicatedAssignment` _(+ RangeHelp)_
- `Gemspec/RubyVersionGlobalsUsage`

### 34. `CodeLength` mixin — 1 cops, 21 tests (Easy)
Port `RuboCop::Cop::CodeLength` once → reuse across all cops in this cluster.

- `Metrics/ModuleLength`

### 35. Gemspec/Bundler — 2 cops, 21 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Bundler/GemFilename` _(+ ConfigurableEnforcedStyle)_
- `Bundler/InsecureProtocolSource`

### 36. `MultilineElementIndentation` mixin — 1 cops, 20 tests (Easy)
Port `RuboCop::Cop::MultilineElementIndentation` once → reuse across all cops in this cluster.

- `Layout/FirstParameterIndentation` _(+ Alignment, ConfigurableEnforcedStyle)_

### 37. Nested constructs — 2 cops, 20 tests (Med)
Topic family, no unique shared mixin — implement individually.

- `Style/NestedModifier` _(+ RangeHelp)_
- `Style/NestedTernaryOperator` _(+ RangeHelp)_

### 38. `StatementModifier` mixin — 2 cops, 17 tests (Easy)
Port `RuboCop::Cop::StatementModifier` once → reuse across all cops in this cluster.

- `Style/MultilineIfModifier` _(+ Alignment)_
- `Style/IfUnlessModifierOfIfUnless`

### 39. Security — 1 cops, 16 tests (Easy)
Topic family, no unique shared mixin — implement individually.

- `Security/Open`

### 40. `SpaceBeforePunctuation` mixin — 2 cops, 15 tests (Easy)
Port `RuboCop::Cop::SpaceBeforePunctuation` once → reuse across all cops in this cluster.

- `Layout/SpaceBeforeSemicolon`
- `Layout/SpaceBeforeComma`

### 41. `DigHelp` mixin — 1 cops, 15 tests (Easy)
Port `RuboCop::Cop::DigHelp` once → reuse across all cops in this cluster.

- `Style/SingleArgumentDig`

### 42. `StringLiteralsHelp` mixin — 1 cops, 13 tests (Easy)
Port `RuboCop::Cop::StringLiteralsHelp` once → reuse across all cops in this cluster.

- `Style/StringLiteralsInInterpolation` _(+ ConfigurableEnforcedStyle, StringHelp)_

### 43. Magic comments/encoding — 1 cops, 13 tests (Easy)
Topic family, no unique shared mixin — implement individually.

- `Style/Encoding` _(+ RangeHelp)_

### 44. `EmptyParameter` mixin — 2 cops, 12 tests (Easy)
Port `RuboCop::Cop::EmptyParameter` once → reuse across all cops in this cluster.

- `Style/EmptyBlockParameter` _(+ RangeHelp)_
- `Style/EmptyLambdaParameter` _(+ RangeHelp)_

### 45. Trailing body/comma — 2 cops, 12 tests (Easy)
Topic family, no unique shared mixin — implement individually.

- `Style/TrailingMethodEndStatement`
- `Lint/TrailingCommaInAttributeDeclaration` _(+ RangeHelp)_

### 46. `OnNormalIfUnless` mixin — 1 cops, 11 tests (Easy)
Port `RuboCop::Cop::OnNormalIfUnless` once → reuse across all cops in this cluster.

- `Style/MultilineIfThen` _(+ RangeHelp)_

### 47. `CheckAssignment` mixin — 1 cops, 10 tests (Easy)
Port `RuboCop::Cop::CheckAssignment` once → reuse across all cops in this cluster.

- `Layout/AssignmentIndentation` _(+ Alignment)_

### 48. `IntegerNode` mixin — 1 cops, 10 tests (Med)
Port `RuboCop::Cop::IntegerNode` once → reuse across all cops in this cluster.

- `Style/NumericLiteralPrefix`

### 49. `SpaceAfterPunctuation` mixin — 1 cops, 9 tests (Easy)
Port `RuboCop::Cop::SpaceAfterPunctuation` once → reuse across all cops in this cluster.

- `Layout/SpaceAfterSemicolon`

### 50. `MinBranchesCount` mixin — 1 cops, 8 tests (Easy)
Port `RuboCop::Cop::MinBranchesCount` once → reuse across all cops in this cluster.

- `Style/HashLikeCase`

### 51. `StringHelp` mixin — 1 cops, 5 tests (Easy)
Port `RuboCop::Cop::StringHelp` once → reuse across all cops in this cluster.

- `Style/CharacterLiteral`
