# Local FAT allocation patch

Base: the published `embedded-sdmmc` 0.10.0 crate, SHA-256
`3fc3ac32bcb461732f1cbc1f199ae3c09f150b18ac6f48a560bef91e0d73ca30`.
Upstream source revision: `10edd5a9d4c5ce6a9a4a474c9893fbda0d8fcf2a`.
The crate's library sources and dependency declarations are retained. Example
and integration-test targets are omitted. Licenses are from that revision.

Brewthink's full-card image-cache test exposed an allocator overflow. Inspection
also found that deleting a directory entry did not release its cluster chain,
which makes a size-limited persistent cache leak storage during eviction.

Changes:

- Bound both FAT16 and FAT32 scans by the volume's cluster count, including the
  final, partially used FAT sector.
- A successful allocation of the last free cluster succeeds without requiring
  another free cluster for the next-allocation hint.
- Propagate I/O failures; only `NotEnoughSpace` permits a wrapped allocation scan.
- Treat stale/overflowing FSInfo counts as unknown and serialize unknown hints.
- Release all clusters after unlinking a closed file or empty directory. A
  failure during release may leave lost clusters, but cannot leave a live
  directory entry pointing at newly reusable data.
- Reuse that release operation for truncation, retaining the original first
  cluster and counting every released tail cluster.

Regression coverage is in Brewthink's in-memory FAT32 tests under
`src/storage/catalog/recovery_tests/`, including full-card failures, cache
capacity/pinning, and write-boundary interruption tests. This is not a claim of
crash-consistent FAT metadata or exhaustive FAT16 testing.
