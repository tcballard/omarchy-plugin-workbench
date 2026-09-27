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
  const context = vm.createContext({ root, ListView: {Contain: 1}, keyCatcher: { forceActiveFocus() { calls.push('focus'); } } });
  for (const name of ['open', 'ensureViewLoaded', 'setViewMode', 'refreshInstalled', 'refreshView', 'refreshUpdates', 'switchSection', 'returnToSections', 'editorOwnsKeyboard', 'activateContent', 'navigateBack', 'moveContent']) {
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

const geometry = vm.createContext({});
vm.runInContext(fs.readFileSync(`${__dirname}/../Navigation.js`, 'utf8').replace('.pragma library', ''), geometry);
function rect(item, x, y, width = 100, height = 30) {
  return {item, x, y, width, height, cx: x + width / 2, cy: y + height / 2};
}
test('arrow navigation prefers the visible row and column over tab order', () => {
  const current = rect('current', 0, 0);
  const right = rect('right', 120, 0);
  const down = rect('down', 0, 60);
  const diagonal = rect('diagonal', 120, 40);
  assert.equal(geometry.nearest([diagonal, down, right], current, 1, 0), 'right');
  assert.equal(geometry.nearest([right, diagonal, down], current, 0, 1), 'down');
  assert.equal(geometry.nearest([current], right, -1, 0), 'current');
  assert.equal(geometry.nearest([current], down, 0, -1), 'current');
});
test('arrow navigation stops at an edge instead of cycling unrelated controls', () => {
  assert.equal(geometry.nearest([rect('right', 120, 0)], rect('current', 0, 0), -1, 0), null);
});
test('Enter opens row details without activating a plugin operation', () => {
  const {root} = panel();
  let opened = 0;
  root.navigationItem = {expanded: false, openDetails() { opened++; }};
  root.activateContent();
  assert.equal(opened, 1);
});
test('changing sections closes details from the previous section', () => {
  const {root} = panel();
  root.detailKey = 'discover:example';
  root.setViewMode('installed');
  assert.equal(root.detailKey, '');
});
test('offscreen row layout completes before keyboard focus moves', () => {
  const {root} = panel();
  const order = [];
  const target = {};
  const feed = {count: 20, positionViewAtIndex(index) { order.push(['scroll', index]); },
    forceLayout() { order.push(['layout']); }, itemAtIndex(index) { order.push(['lookup', index]); return target; }};
  root.navigationItem = {activeFocus: true, workbenchFeed: feed, workbenchIndex: 5};
  root.focusContentItem = item => { assert.equal(item, target); order.push(['focus']); };
  root.moveContent(0, 1);
  assert.equal(feed.currentIndex, 6);
  assert.deepEqual(order, [['scroll', 6], ['layout'], ['lookup', 6], ['focus']]);
});
