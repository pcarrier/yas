/**
 * Place the application catalog without making its idle thousand-row list
 * dominate the switcher. A typed query is different: the matching desktop
 * entry is the launch action and must win Enter over an existing surface.
 */
export function placeApplicationSection<T>(
  sections: T[],
  applications: T,
  searching: boolean,
): void {
  if (searching) sections.unshift(applications);
  else sections.push(applications);
}
