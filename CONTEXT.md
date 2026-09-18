# TypeSafe judgments

TypeSafe interprets supplied evidence using named questions and their criteria.
Applications use the resulting judgments to make decisions.

## Language

**State**: The evidence supplied for one evaluation, expressed as text, an object,
or an array. State describes the situation the questions concern.
_Avoid_: Prompt (which conflates evidence with instructions).

**Question**: A named request for one judgment, with a kind, optional instructions,
and kind-specific criteria. Its name identifies the corresponding answer.

**Criteria**: Interpretations of the alternatives or ordered levels a question
asks the model to evaluate.

**Noul**: A judgment of the probability that a proposition holds, optionally
guided by criteria for true and false.
_Avoid_: Boolean answer (which hides uncertainty).

**Choice**: A judgment among named alternatives, with a selected alternative,
confidence, and probabilities.

**Score**: A judgment over ordered criteria, with a score, confidence, legend,
and probabilities. Changing the order changes the interpretation.

**Answer**: A model's probabilistic judgment for a named question.
_Avoid_: Decision (which belongs to the application).

**Decision**: An application action chosen using answers and application policy,
including any threshold or human review rule.
