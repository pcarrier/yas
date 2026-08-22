# Most terminal frames are tiny

- **Render immediately:** every hot cell is a fixed, renderer-ready 12 bytes.
- **Send the smallest edit:** run, sparse list, bitmap, copy rectangle, or fill rectangle.
- **Compress only when it wins:** transpose cell byte planes; use LZ4 only when it saves ≥8 bytes.
- **Keep rich state separate:** overflow strings, hyperlink tables/runs, line flags; skip optional unknowns.
- **Bound every frame:** chunk sizes stay capped; decoded length is checked before allocation.
- **Never guess a base:** verify keyframe/delta ancestry and sequence windows.
