// TypeScript 7 (the native Go compiler) ships no JS compiler API, but typescript-eslint and
// openapi-typescript still `require("typescript")`. Give those packages their own private
// TypeScript 5.9 while the project-level `typescript` (and `tsc`) stays on 7.x.
const LEGACY_TS = 'npm:typescript@5.9.3';
const NEEDS_LEGACY_TS =
  /^(@typescript-eslint\/|typescript-eslint$|openapi-typescript$|ts-api-utils$)/;

function readPackage(pkg) {
  if (NEEDS_LEGACY_TS.test(pkg.name) && pkg.peerDependencies?.typescript) {
    delete pkg.peerDependencies.typescript;
    pkg.dependencies = { ...pkg.dependencies, typescript: LEGACY_TS };
  }
  return pkg;
}

module.exports = { hooks: { readPackage } };
