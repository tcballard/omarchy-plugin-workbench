const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/../Panel.qml`, 'utf8');

function panel() {
  const calls = [];
  const root = {
    opened: true, busy: false, pendingAction: '', helperPath: '/helper',
    viewMode: 'discover', controller: { show() { calls.push('show'); } },
    marketplaceAttempted: false, portfolioAttempted: false, projectsAttempted: false,
  };
  for (const [property, mode] of Object.entries({marketplaceOpen: 'discover', installedOpen: 'installed', updatesOpen: 'updates', buildOpen: 'build'})) {
    Object.defineProperty(root, property, { get: () => root.viewMode === mode });
  }
  const context = vm.createContext({ root, keyCatcher: { forceActiveFocus() { calls.push('focus'); } } });
  for (const name of ['open', 'ensureViewLoaded', 'setViewMode', 'refreshInstalled', 'refreshView', 'refreshUpdates', 'switchSection', 'returnToSections', 'editorOwnsKeyboard', 'activateContent']) {
    const start = source.indexOf(`  function ${name}(`);
    assert.notEqual(start, -1);
    const end = source.indexOf('\n  }', start) + 4;
    vm.runInContext(source.slice(start, end), context);
    root[name] = context[name];
  }
  for (const [name, flag] of Object.entries({searchMarketplace: 'marketplaceAttempted', loadPortfolio: 'portfolioAttempted', refresh: 'projectsAttempted', refreshMarketplace: '', checkUpdates: ''})) {
    context[name] = root[name] = () => { calls.push(name); if (flag) root[flag] = true; };
  }
  return {root, calls};
}

test('opening Discover reads local cache once, without fetching', () => {
  const {root, calls} = panel();
  root.open(); root.open();
  assert.deepEqual(calls, ['show', 'searchMarketplace', 'show']);
});
test('opening Updates does not fetch Git remotes', () => {
  const {root, calls} = panel();
  root.setViewMode('updates'); root.open();
  assert.deepEqual(calls, ['loadPortfolio', 'show']);
});
test('navigation works while busy and loads the latest tab when idle', () => {
  const {root, calls} = panel();
  root.busy = true;
  root.setViewMode('installed'); root.setViewMode('build');
  assert.equal(root.viewMode, 'build'); assert.deepEqual(calls, []);
  root.busy = false; root.ensureViewLoaded(); root.ensureViewLoaded();
  assert.deepEqual(calls, ['refresh']);
});
test('pending action completion cannot be overwritten by a cache request', () => {
  const {root, calls} = panel();
  root.pendingAction = 'marketplace-install'; root.ensureViewLoaded();
  assert.deepEqual(calls, []);
});
test('explicit refresh fetches but installed action refresh does not', () => {
  const {root, calls} = panel();
  root.refreshView(); root.viewMode = 'updates'; root.refreshView(); root.refreshInstalled();
  assert.deepEqual(calls, ['refreshMarketplace', 'checkUpdates', 'loadPortfolio', 'loadPortfolio']);
});
test('a failed cache attempt does not cause an automatic retry loop', () => {
  const {root, calls} = panel();
  root.marketplaceAttempted = true; root.ensureViewLoaded();
  assert.deepEqual(calls, []);
});

test('section navigation wraps and works during background work', () => {
  const {root} = panel();
  root.busy = true;
  root.switchSection(-1);
  assert.equal(root.viewMode, 'build');
  root.switchSection(1);
  assert.equal(root.viewMode, 'discover');
  assert.equal(root.navigationLevel, 'sections');
});
test('returning from content restores rail focus without closing', () => {
  const {root, calls} = panel();
  root.navigationLevel = 'content'; root.navigationItem = {};
  root.returnToSections();
  assert.equal(root.navigationItem, null);
  assert.equal(root.navigationLevel, 'sections');
  assert.equal(root.opened, true);
  assert.deepEqual(calls, ['focus']);
});
test('text editing owns keys only while the editor is focused', () => {
  const {root} = panel();
  root.navigationItem = {workbenchEditor: true, activeFocus: true};
  assert.equal(root.editorOwnsKeyboard(), true);
  root.navigationItem.activeFocus = false;
  assert.equal(root.editorOwnsKeyboard(), false);
});
test('content activation targets the selected control', () => {
  const {root} = panel();
  let activated = 0;
  root.navigationItem = {activateFromKeyboard() { activated++; }};
  root.activateContent();
  assert.equal(activated, 1);
});
