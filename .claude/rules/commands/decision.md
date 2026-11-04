---
paths:
  - "src/cli/decision.rs"
  - "tests/decision.rs"
---

# `sn decision`

## Modules

- `src/cli/decision.rs` → sn decision list/show/run (decision tables: list is a Table API read of `sys_decision`; show is one GraphQL document over the Decision Builder's scripted `snDtableDesigner`/`snDecisionTable` schemas; run POSTs the Generic API's evaluate route after validating every input against the table's own, because the evaluator silently ignores unknown names, choice labels and record numbers)

**Endpoints** (`decision`): `GET /api/now/table/sys_decision` (list, header); `POST /api/now/graphql` (`snDtableDesigner { decisionInput, decisionCondition, decision }` + `snDecisionTable { answerElement }`); `POST /api/sn_decision_table/generic/evaluate_decision_table?decisionTableId=` (run)

internal and unversioned, all measured on an Australia PDI. The table is `sys_decision` (not `sys_decision_table`). `getDecisionsByDecisionTable` **returns `[]` unless `first_row`/`last_row` are sent** (its resolver calls `chooseWindow(null, null)`), and takes `default_answer: "true"` for the fallback row, so the document reads it under two aliases. Every resolver returns **`null`, not an error, when the caller lacks the roles** (`UserUtil.hasGraphQLAccess`: `decision_table_admin`/`decision_table_reader`/`change_manager`/delegated dev) and `[]` for a nonexistent sys_id — so null is reported as a refusal (exit 2, status 200) and existence comes from the Table API read. Evaluate body: `{testInputs, isFirstMatch, includeDelta, isMultiResultTable, answerElementNames}`; answers `{decision_sys_id: [{value, displayValue}]}` (or `[{name, value, displayValue}]` per element for a multi-result table, `answer_table = sys_decision_multi_result`) — an unknown input name, a choice label or a record number in a reference input all come back as the default decision with exit 0, hence the pre-validation. Plugin absent: `FieldUndefined` on the namespace, and the REST route answers **400** "Requested URI does not represent any resource" (not 404)
