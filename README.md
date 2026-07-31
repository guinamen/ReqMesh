# ReqMesh
Requirements management with a graph-based traceability model

ReqMesh is an open-source requirements management system that treats git as the single source of truth. Actors and Use Cases are plain, version-controlled files — human-readable, diffable, and reviewable through pull/merge requests like any other code change. A graph database builds a read-only, queryable model of the whole requirements landscape (traceability, dependencies, relations) from that git history, kept in sync via a CQRS projection.

Validation rules are graph queries, not hardcoded logic — each project defines its own, and they run automatically against every proposed change before it's merged, using your existing GitLab pipelines. There's no vendor-specific UI to learn: branch, edit, open a merge request, and let CI tell you if the change is structurally sound.

Requirement files follow a versioned, localized schema — the same underlying model can be authored in different human languages (field names and values), so teams aren't locked into a single one.
