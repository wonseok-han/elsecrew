// The product's built-in team names no model (the user picks each one). Smoke
// runs with fixture agents need a complete team, so they save one explicitly as
// the app default before opening a project; the project seeds from it.
module.exports = `(async () => {
  const projects = window.m5Projects;
  const current = (await projects.request({ action: 'getAppRoleDefaults' })).settings;
  const roles = current.roles.map(role => role.modelSelector ? role : {
    ...role,
    modelSelector: role.runtime === 'codex' ? 'fixture-codex' : 'opus',
    reasoningEffort: role.runtime === 'codex' ? 'medium' : null,
  });
  if (roles.every((role, index) => role === current.roles[index])) return false;
  await projects.request({ action: 'updateAppRoleDefaults', expectedRevision: current.revision, roles,
    ...(current.roleDefinitions ? { roleDefinitions: current.roleDefinitions } : {}) });
  return true;
})()`;
