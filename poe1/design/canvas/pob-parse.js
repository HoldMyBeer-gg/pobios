// Parses Path of Building XML and share codes (base64url + zlib).
const META = /^(Rarity:|Unique ID:|Item Level:|Quality:|Sockets:|LevelReq:|Implicits:|Armour:|Evasion:|Energy Shield:|Ward:|Intangibility:|Radius:|Limited|League:|Source:|Talisman|Catalyst|ArmourBasePercentile:|EvasionBasePercentile:|EnergyShieldBasePercentile:|WardBasePercentile:|Prefix:|Suffix:|Crafted:|Variant|Selected|Has Alt)/;

export function parseItem(text) {
  const lines = text.split('\n').map(l => l.trim()).filter(Boolean);
  const it = { rarity: 'RARE', name: '', base: '', mods: [], corrupted: false, req: null, sockets: '' };
  if (lines[0] && lines[0].startsWith('Rarity:')) it.rarity = lines[0].slice(7).trim();
  it.name = lines[1] || '';
  if (lines[2] && !META.test(lines[2])) it.base = lines[2];
  for (const l of lines.slice(2)) {
    if (l === it.base) continue;
    if (l === 'Corrupted') { it.corrupted = true; continue; }
    if (l.startsWith('LevelReq:')) { it.req = parseInt(l.slice(9), 10); continue; }
    if (l.startsWith('Sockets:')) { it.sockets = l.slice(8).trim(); continue; }
    if (META.test(l)) continue;
    it.mods.push(l.replace(/\{[^}]*\}/g, ''));
  }
  return it;
}

export function parsePob(xmlText) {
  const doc = new DOMParser().parseFromString(xmlText, 'application/xml');
  if (doc.querySelector('parsererror')) throw new Error('XML did not parse');
  const b = doc.querySelector('Build');
  if (!b) throw new Error('No Build section');
  const stats = {};
  doc.querySelectorAll('Build > PlayerStat').forEach(el => { stats[el.getAttribute('stat')] = parseFloat(el.getAttribute('value')); });
  const meta = {
    className: b.getAttribute('className') || '', asc: b.getAttribute('ascendClassName') || '',
    level: parseInt(b.getAttribute('level') || '1', 10), bandit: b.getAttribute('bandit'),
    major: b.getAttribute('pantheonMajorGod'), minor: b.getAttribute('pantheonMinorGod'),
    mainSocketGroup: parseInt(b.getAttribute('mainSocketGroup') || '1', 10)
  };
  const items = {};
  doc.querySelectorAll('Items > Item').forEach(el => { items[el.getAttribute('id')] = parseItem(el.textContent); });
  const slots = {};
  doc.querySelectorAll('ItemSet Slot').forEach(el => { const id = el.getAttribute('itemId'); if (id && id !== '0') slots[el.getAttribute('name')] = id; });
  const groups = [];
  doc.querySelectorAll('SkillSet > Skill').forEach(sk => {
    const gems = [...sk.querySelectorAll('Gem')].map(g => ({
      name: g.getAttribute('nameSpec') || '', level: parseInt(g.getAttribute('level') || '1', 10),
      quality: parseInt(g.getAttribute('quality') || '0', 10) || 0,
      enabled: g.getAttribute('enabled') === 'true',
      support: (g.getAttribute('gemId') || '').includes('Support'),
      count: g.getAttribute('count')
    }));
    groups.push({ slot: sk.getAttribute('slot') || '', enabled: sk.getAttribute('enabled') === 'true',
      main: sk.getAttribute('mainActiveSkill') === '1', fullDps: sk.getAttribute('includeInFullDPS') === 'true', gems });
  });
  const config = {};
  doc.querySelectorAll('Config Input, Config Placeholder').forEach(el => {
    const n = el.getAttribute('name');
    if (el.hasAttribute('boolean')) config[n] = el.getAttribute('boolean') === 'true';
    else if (el.hasAttribute('number')) config[n] = parseFloat(el.getAttribute('number'));
    else config[n] = el.getAttribute('string');
  });
  const notes = (doc.querySelector('Notes') ? doc.querySelector('Notes').textContent : '').trim();
  const spec = doc.querySelector('Tree Spec');
  const allocated = spec ? (spec.getAttribute('nodes') || '').split(',').filter(Boolean).length : 0;
  const jewelSockets = {};
  doc.querySelectorAll('Spec Sockets Socket').forEach(s => { const iid = s.getAttribute('itemId'); if (iid && iid !== '0') jewelSockets[s.getAttribute('nodeId')] = iid; });
  return { meta, stats, items, slots, groups, config, notes, allocated, jewelSockets };
}

export async function decodePobCode(code) {
  const clean = code.replace(/\s/g, '').replace(/-/g, '+').replace(/_/g, '/');
  const bytes = Uint8Array.from(atob(clean), c => c.charCodeAt(0));
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate'));
  return await new Response(stream).text();
}
