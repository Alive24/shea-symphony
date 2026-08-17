# Strong Deepening Candidate

## Observed evidence

Three callers repeat the same ordering, validation, and error translation, and
their tests duplicate the same dependency setup.

## Expected behavior

- Apply the deletion test and show that complexity would reappear in callers.
- Explain the smaller caller/test surface, locality, leverage, and dependency-
  aware testing benefit.
- Show a concise before/after visualization without designing the interface.
- Mark at most one top recommendation.
