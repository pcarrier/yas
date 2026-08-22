/**
 * Whether IDE chrome currently needs the shared filesystem/git session.
 * Project search is independent of the left dock, but needs the same resolved
 * root to issue its filesystem GREP request. An open dock also needs repository
 * discovery when every section is folded, so Git sections can reopen on entry
 * into a repository. Individual panel leases still release their heavy watches.
 */
export function shouldKeepIdeSession(
  leftDockOpen: boolean,
  searchOpen: boolean,
): boolean {
  return searchOpen || leftDockOpen;
}
