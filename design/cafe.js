/*
 * Token Cafe: a 480x272 pixel scene of the coding-agent sessions on this
 * machine, drawn as patrons of a JRPG-style net cafe.
 *
 * Pure JS with no DOM: it renders into an RGBA buffer, so the same file runs
 * in a browser canvas and under node for previews. Every colour is quantized
 * to RGB565 like the ILI9341 panel, sprites are palette strings (one char per
 * pixel, '.' transparent) so they convert 1:1 to 4bpp C arrays, and the scene
 * logic uses integer pixel positions and a millisecond clock, the way the
 * ESP32-S3 firmware will.
 */
(function (root) {
'use strict';

const W = 480, H = 272;

/* ---------------------------------------------------------------- colour */

function q(hex) {
    const v = parseInt(hex.slice(1), 16);
    const r = (v >> 19) & 31, g = (v >> 10) & 63, b = (v >> 3) & 31;
    return [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)];
}
function q3(r, g, b) {
    r = Math.max(0, Math.min(255, r | 0)) >> 3;
    g = Math.max(0, Math.min(255, g | 0)) >> 2;
    b = Math.max(0, Math.min(255, b | 0)) >> 3;
    return [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)];
}

/* Fixed colours every sprite may use. */
const BASE = {
    K: q('#1a1222'),   // outline
    W: q('#f4f2ec'),   // white
    w: q('#b8b4c4'),   // white shade
    R: q('#f08a8a'),   // blush
    M: q('#7a2032'),   // open mouth
    X: q('#e84040'),   // alarm red
    Y: q('#f8d050'),   // yellow
    y: q('#b88a20'),   // yellow shade
    O: q('#c8643c'),   // terracotta
    o: q('#8a3c22'),   // terracotta shade
    L: q('#58b048'),   // leaf
    l: q('#2f7030'),   // leaf shade
    B: q('#26232e'),   // chair cushion
    b: q('#34303e'),   // chair cushion light
    Z: q('#9ad8f8'),   // sleep Z
    G: q('#7ae0c8'),   // can green
    g: q('#3a9a88'),   // can green shade
    N: q('#e8c890'),   // noodles
};

/* Scene colours. */
const C = {
    ceil: q('#140e1c'),
    wallA: q('#3a2a4c'), wallB: q('#433156'), wallDot: q('#4e3a62'),
    rail: q('#7a5238'), railHi: q('#9a6c4a'),
    wain: q('#4a2f24'), wainHi: q('#5a3a2c'), wainLo: q('#33201a'),
    base: q('#1e1418'),
    sky0: q('#141a3a'), sky1: q('#1c2650'), sky2: q('#2a3468'),
    star: q('#f8f0c0'), starDim: q('#8a88a8'), moon: q('#f4e8b0'),
    city: q('#0c0e1e'), cityB: q('#151a30'), lit: q('#f8d070'), litB: q('#c87848'),
    frame: q('#6a4630'), frameHi: q('#8c6040'), sill: q('#a07050'),
    neon: q('#ff6cb4'), neonCore: q('#ffd2ea'), neonGlow: q('#7a2a64'), neonOff: q('#5a3050'),
    cyan: q('#68e8ff'), cyanGlow: q('#1e5070'),
    chalk: q('#24342c'), chalkHi: q('#e8ecdc'), chalkDim: q('#a0b0a0'), chalkGold: q('#f0c860'),
    clockFace: q('#ece4cc'), clockRim: q('#9a7048'),
    floorA: q('#2c3248'), floorB: q('#30374f'), floorLine: q('#232839'), floorHi: q('#383f58'),
    runner: q('#5a2430'), runnerHi: q('#702e3a'), runnerEdge: q('#c8a050'),
    deskTop: q('#c8925a'), deskHi: q('#e0b07a'), deskGrain: q('#b07c48'),
    deskFront: q('#8a5832'), deskFrontLo: q('#6a4024'), deskEdge: q('#f0c890'),
    plate: q('#e8dcc0'), plateLo: q('#a89470'), plateClaude: q('#f2d9c8'), plateCodex: q('#d4dde6'),
    win0: q('#2c58c8'), win1: q('#0e1a5a'), winBorder: q('#eceaf4'), winBorderLo: q('#8a90b8'),
    winOuter: q('#06081c'), text: q('#f4f4f8'), textShadow: q('#080a28'), textDim: q('#a8b0d8'),
    gold: q('#f8d860'),
};

/* Per-agent laptops. */
const LAPTOP = {
    claude: { L: q('#ece2d2'), l: q('#c4b6a2'), A: q('#d97757'), a: q('#f4b090') },
    codex:  { L: q('#3e4250'), l: q('#2a2d38'), A: q('#e8f0f4'), a: q('#6ae0d0') },
};

/* Avatar looks, picked from a hash of the session id. */
const SKINS = [['#f8d4b0', '#d8a07c'], ['#e8b080', '#c07c50'], ['#b07450', '#80502e']];
const HAIRS = [
    ['#4a3028', '#2c1c18', '#6e4a3a'], ['#e8c058', '#b08828', '#f8e090'],
    ['#c84838', '#8a2a24', '#e87060'], ['#4a68c8', '#2e4088', '#7a98e8'],
    ['#dcdce8', '#9a9aae', '#ffffff'], ['#5aa848', '#367028', '#88d070'],
    ['#e070b0', '#a04480', '#f8a0d0'], ['#26242e', '#121016', '#46424e'],
];
const SHIRTS = [
    ['#d86a40', '#a04428'], ['#4a7ad0', '#30529a'], ['#4aa868', '#2e7a46'],
    ['#9058c0', '#643c92'], ['#e0b040', '#a88020'], ['#e8e6f0', '#aaa8bc'],
    ['#d04868', '#962e48'], ['#3c414e', '#262a34'],
];
const PANTS = ['#2c3450', '#3a2c28', '#1e1e26', '#44506a'];
const CHAIRS = [['#b83c3c', '#7a2626'], ['#3a6ab8', '#24467a'], ['#3a9a5a', '#246a3a'], ['#8a4ab8', '#5a2c7a']];
const HAIR_STYLES = ['short', 'long', 'bun'];

function hash(s) {
    let h = 2166136261;
    for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
    return h >>> 0;
}

function lookFor(id) {
    const h = hash(id);
    const sk = SKINS[h % 3], hr = HAIRS[(h >>> 3) % 8], sh = SHIRTS[(h >>> 7) % 8];
    return {
        S: q(sk[0]), s: q(sk[1]),
        H: q(hr[0]), h: q(hr[1]), J: q(hr[2]),
        C: q(sh[0]), c: q(sh[1]), V: q('#f4f2ec'),
        P: q(PANTS[(h >>> 11) % 4]), F: q('#3a2420'),
        style: HAIR_STYLES[(h >>> 20) % HAIR_STYLES.length],
        variant: (h >>> 13) % 3,          // which idle pose this agent prefers
        phase: (h >>> 16) % 9000,         // desynchronises the agents' timers
    };
}

/* ---------------------------------------------------------------- buffer */

let FB = null;

function px(x, y, c) {
    if (x < 0 || y < 0 || x >= W || y >= H) return;
    const i = (y * W + x) << 2;
    FB[i] = c[0]; FB[i + 1] = c[1]; FB[i + 2] = c[2]; FB[i + 3] = 255;
}
function rect(x, y, w, h, c) {
    for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) px(i, j, c);
}
function darken(x, y, w, h, k) {
    for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) {
        if (i < 0 || j < 0 || i >= W || j >= H) continue;
        const o = (j * W + i) << 2;
        const c = q3(FB[o] * k, FB[o + 1] * k, FB[o + 2] * k);
        FB[o] = c[0]; FB[o + 1] = c[1]; FB[o + 2] = c[2];
    }
}

/* --------------------------------------------------------------- sprites */

function S(name, rows) {
    const w = rows[0].length;
    rows.forEach((r, i) => {
        if (r.length !== w) throw new Error(`sprite ${name} row ${i} is ${r.length} wide, expected ${w}: "${r}"`);
    });
    return { name, w, h: rows.length, rows };
}

function blit(s, x, y, pal, flip) {
    for (let j = 0; j < s.h; j++) {
        const row = s.rows[j];
        for (let i = 0; i < s.w; i++) {
            const ch = row[flip ? s.w - 1 - i : i];
            if (ch === '.') continue;
            const c = (pal && pal[ch]) || BASE[ch];
            if (c) px(x + i, y + j, c);
        }
    }
}

function blitScaled(s, x, y, pal, k, clip) {
    for (let j = 0; j < s.h; j++) for (let i = 0; i < s.w; i++) {
        const ch = s.rows[j][i];
        if (ch === '.') continue;
        const c = (pal && pal[ch]) || BASE[ch];
        if (!c) continue;
        for (let v = 0; v < k; v++) for (let u = 0; u < k; u++) {
            const X = x + i * k + u, Y = y + j * k + v;
            if (clip && (X < clip[0] || Y < clip[1] || X >= clip[2] || Y >= clip[3])) continue;
            px(X, Y, c);
        }
    }
}

/* Character sprites are composed from parts on a 22x30 canvas whose 16x24
 * "base" box (where a standing body lives) starts at (BX, BY); the margins
 * hold raised arms, mugs and elbows. */
const BX = 3, BY = 6, CW = 22, CH = 30;

function compose(name, parts) {
    const g = Array.from({ length: CH }, () => Array(CW).fill('.'));
    for (const [p, x, y] of parts) {
        for (let j = 0; j < p.h; j++) for (let i = 0; i < p.w; i++) {
            const ch = p.rows[j][i];
            if (ch === '.') continue;
            const gx = BX + x + i, gy = BY + y + j;
            if (gx < 0 || gy < 0 || gx >= CW || gy >= CH) throw new Error(`${name}: part ${p.name} leaves the canvas`);
            g[gy][gx] = ch;
        }
    }
    return S(name, g.map(r => r.join('')));
}

/* Character palette keys: H h J hair (base, shade, shine), S s skin,
 * C c shirt, V collar, P pants, F shoes; K W R M come from BASE. */

const HEAD = {
    open: S('head.open', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSSSSSSSSShK',
        'KhSSKSSSSKSShK',
        'KhSSKSSSSKSShK',
        '.KSRSSSSSSRSK.',
        '..KsSSSSSSsK..',
        '...KKKKKKKK...',
    ]),
    down: S('head.down', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSSSSSSSSShK',
        'KhSSSSSSSSSShK',
        'KhSSKSSSSKSShK',
        '.KSRSSSSSSRSK.',
        '..KsSSSSSSsK..',
        '...KKKKKKKK...',
    ]),
    up: S('head.up', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSSKSSSSKShK',
        'KhSSSKSSSSKShK',
        'KhSSSSSSSSSShK',
        '.KSRSSSSSSRSK.',
        '..KsSSSSSSsK..',
        '...KKKKKKKK...',
    ]),
    closed: S('head.closed', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSSSSSSSSShK',
        'KhSSSSSSSSSShK',
        'KhSKKSSSSKKShK',
        '.KSRSSSSSSRSK.',
        '..KsSSSSSSsK..',
        '...KKKKKKKK...',
    ]),
    sleep: S('head.sleep', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSSSSSSSSShK',
        'KhSSSSSSSSSShK',
        'KhSKKSSSSKKShK',
        '.KSRSSSSSSRSK.',
        '..KsSSMSSSsK..',
        '...KKKKKKKK...',
    ]),
    shout: S('head.shout', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHSHHSSHHShhK',
        'KhSSKSSSSKSShK',
        'KhSSKSSSSKSShK',
        'KhSSKSSSSKSShK',
        '.KSRSSMMSSRSK.',
        '..KsSSMMSSsK..',
        '...KKKKKKKK...',
    ]),
    back: S('head.back', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHhHK',
        'KHhHHHHHHHHHhK',
        'KHHHHHhHHhHHHK',
        'KhHHHHHHHHHHhK',
        'KhHHhHHHHHHHhK',
        'KhHHHHHHhHHHhK',
        'KhhHHHHHHHHhhK',
        '.KhhHHHHHHhhK.',
        '..KhhhhhhhhK..',
        '...KKKKKKKK...',
    ]),
    side: S('head.side', [
        '...KKKKKKKK...',
        '..KHHHJJHHHK..',
        '.KHHJJHHHHHHK.',
        'KHHHHHHHHHHHHK',
        'KHhHHHHHHHHHHK',
        'KHHHHHhHHHHhHK',
        'KhHHHHHHSHHSHK',
        'KhHHHHHSSSSSSK',
        'KhHHHHSSSSKSSK',
        'KhhHHHSSSSKSSK',
        '.KhhHSSSSSSRK.',
        '..KhhKsSSSsK..',
        '...KKKKKKKK...',
    ]),
};

const TORSO = {
    front: S('torso.front', [
        '....KKVVVVKK....',
        '...KCCCCCCCCK...',
        '..KCKCCCCCCKCK..',
        '..KCKCCCCCCKCK..',
        '..KcKCCCCCCKcK..',
        '..KSKccccccKSK..',
    ]),
    back: S('torso.back', [
        '....KKCCCCKK....',
        '...KCCCCCCCCK...',
        '..KCKCCCCCCKCK..',
        '..KCKCCCCCCKCK..',
        '..KcKcCCCCcKcK..',
        '..KSKccccccKSK..',
    ]),
    side: S('torso.side', [
        '.....KKCCCK.....',
        '....KCCCCCCK....',
        '....KCCCKCCK....',
        '....KCCCKCCK....',
        '....KccKSKcK....',
        '....KKKKKKKK....',
    ]),
    sit: S('torso.sit', [
        '....KKVVVVKK....',
        '...KCCCCCCCCK...',
        '..KCCCCCCCCCCK..',
        '..KCCCCCCCCCCK..',
        '..KCCCCCCCCCCK..',
        '..KCCCCCCCCCCK..',
        '..KcCCCCCCCCcK..',
        '..KcCCCCCCCCcK..',
    ]),
};

const LEGS = {
    stand: S('legs.stand', [
        '...KKPPPPPPKK...',
        '....KPPPPPPK....',
        '....KPPKKPPK....',
        '....KFFKKFFK....',
        '....KKKKKKKK....',
    ]),
    walkA: S('legs.walkA', [
        '...KKPPPPPPKK...',
        '....KPPPPPPK....',
        '....KPPKKFFK....',
        '....KFFKKKKK....',
        '....KKKK........',
    ]),
    walkB: S('legs.walkB', [
        '...KKPPPPPPKK...',
        '....KPPPPPPK....',
        '....KFFKKPPK....',
        '....KKKKKFFK....',
        '........KKKK....',
    ]),
    sideStand: S('legs.sideStand', [
        '.....KPPPPK.....',
        '.....KPPPPK.....',
        '.....KPPPPK.....',
        '.....KFFFFFK....',
        '.....KKKKKKK....',
    ]),
    sideWalk: S('legs.sideWalk', [
        '.....KPPPPK.....',
        '....KPPKKPPK....',
        '...KPPK..KPPK...',
        '..KFFFK..KFFFK..',
        '..KKKKK..KKKKK..',
    ]),
};

const ARM = {
    left: S('arm.left', [
        '.KK.',
        'KCCK',
        'KCcK',
        'KCcK',
        'KSSK',
        '.KK.',
    ]),
    right: S('arm.right', [
        '.KK.',
        'KCCK',
        'KcCK',
        'KcCK',
        'KSSK',
        '.KK.',
    ]),
    upA: S('arm.upA', [
        '.KK.',
        'KSSK',
        'KSSK',
        'KSSK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KCCK',
        'KcCK',
        'KcCK',
        'KcCK',
        '.KK.',
    ]),
    upB: S('arm.upB', [
        '...KK',
        '..KSSK',
        '..KSSK',
        '.KSSK.',
        '.KCCK.',
        '.KCCK.',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KCCK..',
        'KcCK..',
        'KcCK..',
        'KcCK..',
        '.KK...',
    ].map(r => r.padEnd(6, '.'))),
    chin: S('arm.chin', [
        '.KKKK.',
        'KSSSSK',
        '.KSSK.',
        '..KCK.',
        '..KCCK',
        '..KcCK',
        '..KcCK',
        '...KK.',
    ]),
    mugUp: S('arm.mugUp', [
        '.KKKKK..',
        '.KWWWKK.',
        '.KWWWK.K',
        'KSWWWKK.',
        'KSSWWK..',
        '.KSKK...',
        '..KCK...',
        '..KCCK..',
        '..KcCK..',
        '..KcCK..',
        '...KK...',
    ]),
    mugDown: S('arm.mugDown', [
        '.KKKKK..',
        '.KWWWKK.',
        'KSWWWK.K',
        'KSSWWKK.',
        '.KKKK...',
        '..KCK...',
        '..KcK...',
        '...K....',
    ]),
    behindL: S('arm.behindL', [
        '.KK....',
        'KCCK...',
        'KCCCK..',
        '.KCCCK.',
        '..KCCCK',
        '...KCCK',
        '....KCK',
        '.....KK',
    ]),
};
ARM.behindR = S('arm.behindR', ARM.behindL.rows.map(r => r.split('').reverse().join('')));
ARM.folded = S('arm.folded', [
    '.KKKKKKKKKKKKKK.',
    'KCCCCCKSSKCCCCCK',
    'KCCCCKSSSSKCCCCK',
    'KccccKKKKKKccccK',
    '.KKKKKKKKKKKKKK.',
]);

HEAD.yawn = S('head.yawn', HEAD.closed.rows.map((r, i) => (i === 10 ? '.KSRSSMMSSRSK.' : i === 11 ? '..KsSSMMSSsK..' : r)));

/* Hair styles are overlays stamped over the head (and the shoulders). */
const HAIR = {
    longF: S('hair.longF', [
        ...Array(10).fill('..............'),
        'KhH........HhK',
        'KhHK......KHhK',
        'KhHK......KHhK',
        'KhhK......KhhK',
        '.KK........KK.',
    ]),
    longB: S('hair.longB', [
        ...Array(10).fill('..............'),
        'KhHHHHHHHHHHhK',
        'KhHHHHHHHHHHhK',
        'KhhHHHHHHHHhhK',
        '.KhhhhhhhhhhK.',
        '..KKKKKKKKKK..',
    ]),
    longS: S('hair.longS', [
        ...Array(10).fill('..............'),
        'KhHH..........',
        'KhHHK.........',
        'KhHHK.........',
        'KhhK..........',
        '.KK...........',
    ]),
    bun: S('hair.bun', [
        '....KKKKKK....',
        '...KHHJJHHK...',
        '...KHHHHhHK...',
    ]),
};

function head(h, x, y, style, view = 'front') {
    const parts = [[h, x, y]];
    if (style === 'long') parts.push([view === 'back' ? HAIR.longB : view === 'side' ? HAIR.longS : HAIR.longF, x, y]);
    if (style === 'bun') parts.push([HAIR.bun, x, y - 3]);
    return parts;
}

function buildPoses(st) {
    const T = TORSO.sit;
    const P = {};
    const stand = (name, h, torso, legs, view) => compose(name, [[torso, 0, 13], [legs, 0, 19], ...head(h, 1, 0, st, view)]);
    P.standD = stand('standD', HEAD.open, TORSO.front, LEGS.stand);
    P.walkD = [stand('walkDA', HEAD.open, TORSO.front, LEGS.walkA), stand('walkDB', HEAD.open, TORSO.front, LEGS.walkB)];
    P.standU = stand('standU', HEAD.back, TORSO.back, LEGS.stand, 'back');
    P.walkU = [stand('walkUA', HEAD.back, TORSO.back, LEGS.walkA, 'back'), stand('walkUB', HEAD.back, TORSO.back, LEGS.walkB, 'back')];
    P.standS = stand('standS', HEAD.side, TORSO.side, LEGS.sideStand, 'side');
    P.walkS = [stand('walkSA', HEAD.side, TORSO.side, LEGS.sideWalk, 'side'), stand('walkSB', HEAD.side, TORSO.side, LEGS.sideStand, 'side')];
    P.type = [
        compose('typeA', [[T, 0, 13], [ARM.left, 0, 15], [ARM.right, 12, 16], ...head(HEAD.down, 1, 0, st)]),
        compose('typeB', [[T, 0, 13], [ARM.left, 0, 16], [ARM.right, 12, 15], ...head(HEAD.down, 1, 1, st)]),
    ];
    P.think = compose('think', [[T, 0, 13], [ARM.left, 0, 15], ...head(HEAD.up, 1, 0, st), [ARM.chin, 8, 10]]);
    P.raise = [
        compose('raiseA', [[T, 0, 13], [ARM.left, 0, 15], ...head(HEAD.shout, 1, 0, st), [ARM.upA, 14, -5]]),
        compose('raiseB', [[T, 0, 13], [ARM.left, 0, 15], ...head(HEAD.shout, 1, 0, st), [ARM.upB, 13, -5]]),
    ];
    P.stretch = compose('stretch', [[T, 0, 13], ...head(HEAD.yawn, 1, 0, st), [ARM.behindL, -3, 6], [ARM.behindR, 12, 6]]);
    P.sip = [
        compose('sipUp', [[T, 0, 13], [ARM.left, 0, 15], ...head(HEAD.closed, 1, 0, st), [ARM.mugUp, 7, 7]]),
        compose('sipDown', [[T, 0, 13], [ARM.left, 0, 15], ...head(HEAD.open, 1, 0, st), [ARM.mugDown, 8, 12]]),
    ];
    P.sleep = [
        compose('sleepA', [[T, 0, 14], ...head(HEAD.back, 1, 8, st, 'back'), [ARM.folded, 0, 17]]),
        compose('sleepB', [[T, 0, 14], ...head(HEAD.back, 1, 9, st, 'back'), [ARM.folded, 0, 17]]),
    ];
    return P;
}
const POSES = {};
for (const st of HAIR_STYLES) POSES[st] = buildPoses(st);
const POSE = POSES.short;

/* ------------------------------------------------------------ furniture */

const CHAIR = S('chair', [
    '....KKKKKKKKKKKK....',
    '...KAAAAAAAAAAAAK...',
    '..KAAaaaaaaaaaaAAK..',
    '..KAaKKKKKKKKKKaAK..',
    '..KAaKBBBBBBBBKaAK..',
    '..KAaKBbBBBBbBKaAK..',
    '..KAaKBBBBBBBBKaAK..',
    '..KAaKBBBAABBBKaAK..',
    '..KAaKBBBAABBBKaAK..',
    '..KAaKBBBBBBBBKaAK..',
    '..KAaKBBBBBBBBKaAK..',
    '..KAaKBbBBBBbBKaAK..',
    '..KAaKBBBBBBBBKaAK..',
    '.KKKaKBBBBBBBBKaKKK.',
    'KAAKKKBBBBBBBBKKKAAK',
    'KaaaaKKKKKKKKKKaaaaK',
    'KKKKKBBBBBBBBBBKKKKK',
    '....KbbbbbbbbbbK....',
    '....KKKKKKKKKKKK....',
]);

const LID = S('lid', [
    'KKKKKKKKKKKKK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KLLLLLLLLLLLK',
    'KlllllllllllK',
    'KKKKKKKKKKKKK',
]);

/* Claude's lid shows the CLI's asterisk spinner; Codex's a blinking prompt. */
const SPARK = [
    S('spark0', ['.......', '.......', '.......', '...A...', '.......', '.......', '.......']),
    S('spark1', ['.......', '.......', '..A.A..', '...A...', '..A.A..', '.......', '.......']),
    S('spark2', ['.......', '.A.A.A.', '..AAA..', '.AAaAA.', '..AAA..', '.A.A.A.', '.......']),
    S('spark3', ['A..A..A', '.A.A.A.', '..AAA..', 'AAAaAAA', '..AAA..', '.A.A.A.', 'A..A..A']),
];
const PROMPT = [
    S('prompt0', ['.......', '.A.....', '..A....', '...A...', '..A....', '.A.....', '.......']),
    S('prompt1', ['.......', '.A.....', '..A....', '...A...', '..A....', '.A..aaa', '.......']),
];

const PROPS = {
    duck: S('duck', [
        '...KKK..',
        '..KYYYK.',
        '..KYKYKK',
        'K.KYYYOK',
        'KYKYYYKK',
        'KYYYYYYK',
        'KyYYYYyK',
        '.KKKKKK.',
    ]),
    plant: S('plant', [
        '...KK...',
        '..KLLK.K',
        '.KLlLKKL',
        'KLLKLLlK',
        '.KlLLLK.',
        'KLKLlKLK',
        '.KKLLKK.',
        '.KKKKKK.',
        '.KOOOOK.',
        '.KOooOK.',
        '..KOOK..',
        '..KKKK..',
    ]),
    noodles: S('noodles', [
        '.KKKKK.',
        'KWWWWWK',
        'KXXXXXK',
        'KWWWWWK',
        '.KWwWK.',
        '.KWwWK.',
        '..KKK..',
    ]),
    can: S('can', [
        'KKKK',
        'KwWK',
        'KGGK',
        'KgGK',
        'KGGK',
        'KgGK',
        'KKKK',
    ]),
    books: S('books', [
        '.KKKKKKKK.',
        'KOOOOOOOOK',
        'KKKKKKKKKK',
        'KGGGGGGGGK',
        'KKKKKKKKKK',
        '.KYYYYYYYK',
        '.KKKKKKKKK',
    ]),
};

const BUBBLE = {
    bang: S('bang', [
        '.KKKKKKKK.',
        'KWWWWWWWWK',
        'KWWWXXWWWK',
        'KWWWXXWWWK',
        'KWWWXXWWWK',
        'KWWWXXWWWK',
        'KWWWWWWWWK',
        'KWWWXXWWWK',
        'KWWWWWWWWK',
        '.KKWWKKKK.',
        '..KWK.....',
        '..KK......',
    ]),
    ask: S('ask', [
        '.KKKKKKKK.',
        'KWWWWWWWWK',
        'KWWXXXXWWK',
        'KWXXWWXXWK',
        'KWWWWXXWWK',
        'KWWWXXWWWK',
        'KWWWWWWWWK',
        'KWWWXXWWWK',
        'KWWWWWWWWK',
        '.KKWWKKKK.',
        '..KWK.....',
        '..KK......',
    ]),
    think: S('think', [
        '..KKKKKKKKK..',
        '.KWWWWWWWWWK.',
        'KWWWWWWWWWWWK',
        'KWWKWWKWWKWWK',
        'KWWWWWWWWWWWK',
        '.KWWWWWWWWWK.',
        '..KKKKKKKKK..',
        '..KK.........',
        '.KWWK........',
        '..KK.........',
    ]),
};

const KEYS = [
    S('k0', ['A.A', '.A.', 'A.A']), S('k1', ['.A.', 'AAA', '.A.']), S('k2', ['A..', '.A.', 'A..']),
    S('k3', ['AAA', '...', 'AAA']), S('k4', ['.A', 'A.', '.A']),
];
const ZSMALL = S('zs', ['ZZZ', '.Z.', 'ZZZ']);
const ZBIG = S('zb', ['ZZZZ', '..Z.', '.Z..', 'ZZZZ']);

const CURSOR = S('cursor', [
    '..KKK.......',
    '.KWWWKKKKKK.',
    'KWWWWWWWWWWK',
    'KWWWWWKKKKK.',
    'KWWWWWWK....',
    'KwWWWWKK....',
    'KwwWWWWK....',
    '.KKKKKK.....',
]);

const ICON = {
    laptop: S('i.laptop', [
        '.KKKKKKKK.',
        '.KAAAAAAK.',
        '.KAaaaaAK.',
        '.KAaaaaAK.',
        '.KAAAAAAK.',
        'KKKKKKKKKK',
        'KWWWWWWWWK',
        'KKKKKKKKKK',
    ]),
    cup: S('i.cup', [
        '..W.W...',
        '.W.W....',
        'KKKKKK..',
        'KWWWWKKK',
        'KWWWWK.K',
        'KWWWWKKK',
        'KwWWWK..',
        '.KKKK...',
    ]),
    bang: S('i.bang', [
        '.KKKK.',
        'KXXXXK',
        'KXXXXK',
        'KXXXXK',
        '.KXXK.',
        '.KXXK.',
        '..KK..',
        '.KXXK.',
        '.KKKK.',
    ]),
};

/* ----------------------------------------------------------------- font */

/* 5x7 capitals, digits and the punctuation names use. */
const GLYPH_SRC = {
    'A': '.###.|#...#|#...#|#####|#...#|#...#|#...#', 'B': '####.|#...#|#...#|####.|#...#|#...#|####.',
    'C': '.###.|#...#|#....|#....|#....|#...#|.###.', 'D': '####.|#...#|#...#|#...#|#...#|#...#|####.',
    'E': '#####|#....|#....|####.|#....|#....|#####', 'F': '#####|#....|#....|####.|#....|#....|#....',
    'G': '.###.|#...#|#....|#.###|#...#|#...#|.####', 'H': '#...#|#...#|#...#|#####|#...#|#...#|#...#',
    'I': '.###.|..#..|..#..|..#..|..#..|..#..|.###.', 'J': '..###|...#.|...#.|...#.|...#.|#..#.|.##..',
    'K': '#...#|#..#.|#.#..|##...|#.#..|#..#.|#...#', 'L': '#....|#....|#....|#....|#....|#....|#####',
    'M': '#...#|##.##|#.#.#|#.#.#|#...#|#...#|#...#', 'N': '#...#|#...#|##..#|#.#.#|#..##|#...#|#...#',
    'O': '.###.|#...#|#...#|#...#|#...#|#...#|.###.', 'P': '####.|#...#|#...#|####.|#....|#....|#....',
    'Q': '.###.|#...#|#...#|#...#|#.#.#|#..#.|.##.#', 'R': '####.|#...#|#...#|####.|#.#..|#..#.|#...#',
    'S': '.####|#....|#....|.###.|....#|....#|####.', 'T': '#####|..#..|..#..|..#..|..#..|..#..|..#..',
    'U': '#...#|#...#|#...#|#...#|#...#|#...#|.###.', 'V': '#...#|#...#|#...#|#...#|#...#|.#.#.|..#..',
    'W': '#...#|#...#|#...#|#.#.#|#.#.#|#.#.#|.#.#.', 'X': '#...#|#...#|.#.#.|..#..|.#.#.|#...#|#...#',
    'Y': '#...#|#...#|.#.#.|..#..|..#..|..#..|..#..', 'Z': '#####|....#|...#.|..#..|.#...|#....|#####',
    '0': '.###.|#...#|#..##|#.#.#|##..#|#...#|.###.', '1': '..#..|.##..|..#..|..#..|..#..|..#..|.###.',
    '2': '.###.|#...#|....#|...#.|..#..|.#...|#####', '3': '#####|...#.|..#..|...#.|....#|#...#|.###.',
    '4': '...#.|..##.|.#.#.|#..#.|#####|...#.|...#.', '5': '#####|#....|####.|....#|....#|#...#|.###.',
    '6': '..##.|.#...|#....|####.|#...#|#...#|.###.', '7': '#####|....#|...#.|..#..|.#...|.#...|.#...',
    '8': '.###.|#...#|#...#|.###.|#...#|#...#|.###.', '9': '.###.|#...#|#...#|.####|....#|...#.|.##..',
    ' ': '.....|.....|.....|.....|.....|.....|.....', '-': '.....|.....|.....|.###.|.....|.....|.....',
    '.': '.....|.....|.....|.....|.....|.##..|.##..', ':': '.....|.##..|.##..|.....|.##..|.##..|.....',
    '!': '..#..|..#..|..#..|..#..|..#..|.....|..#..', '?': '.###.|#...#|....#|...#.|..#..|.....|..#..',
    '/': '.....|....#|...#.|..#..|.#...|#....|.....', ',': '.....|.....|.....|.....|.##..|..#..|.#...',
    "'": '..#..|..#..|.#...|.....|.....|.....|.....', '_': '.....|.....|.....|.....|.....|.....|#####',
    '*': '.....|.....|..#..|.###.|..#..|.....|.....', '>': '#....|.#...|..#..|...#.|..#..|.#...|#....',
    '(': '...#.|..#..|.#...|.#...|.#...|..#..|...#.', ')': '.#...|..#..|...#.|...#.|...#.|..#..|.#...',
    '+': '.....|..#..|..#..|#####|..#..|..#..|.....', '#': '.#.#.|.#.#.|#####|.#.#.|#####|.#.#.|.#.#.',
    '$': '..#..|.####|#.#..|.###.|..#.#|####.|..#..',
    "<": "....#|...#.|..#..|.#...|..#..|...#.|....#",
    "^": "..#..|.#.#.|#...#|.....|.....|.....|.....",
    "\"": ".#.#.|.#.#.|.....|.....|.....|.....|.....",
    "\\": "#....|.#...|..#..|...#.|....#|.....|.....",
    "{": "...##|..#..|..#..|.#...|..#..|..#..|...##",
    "}": "##...|..#..|..#..|...#.|..#..|..#..|##...",
    "[": ".###.|.#...|.#...|.#...|.#...|.#...|.###.",
    "]": ".###.|...#.|...#.|...#.|...#.|...#.|.###.",
    "=": ".....|.....|#####|.....|#####|.....|.....",
    ";": ".....|..#..|.....|.....|..#..|..#..|.#...",
    "|": "..#..|..#..|..#..|..#..|..#..|..#..|..#..",
    "&": ".##..|#..#.|#.#..|.##..|#.#.#|#..#.|.##.#",
    "%": "##..#|##.#.|...#.|..#..|.#...|#..##|...##",
    "@": ".###.|#...#|#.###|#.#.#|#.###|#....|.####",
    "`": ".#...|..#..|.....|.....|.....|.....|.....",
    "~": ".....|.....|.#..#|#.##.|.....|.....|.....",

};
const GLYPH = {};
for (const [ch, rows] of Object.entries(GLYPH_SRC)) {
    const g = rows.split('|');
    if (g.length !== 7 || g.some(r => r.length !== 5)) throw new Error(`glyph ${ch} is not 5x7`);
    GLYPH[ch] = g;
}

function textWidth(s, k = 1) { return s.length ? (s.length * 6 - 1) * k : 0; }

function text(s, x, y, c, k = 1, shadow = C.textShadow) {
    s = s.toUpperCase();
    for (const pass of shadow ? [0, 1] : [1]) {
        const col = pass ? c : shadow;
        const o = pass ? 0 : 1;
        for (let n = 0; n < s.length; n++) {
            const g = GLYPH[s[n]] || GLYPH['?'];
            for (let j = 0; j < 7; j++) for (let i = 0; i < 5; i++) {
                if (g[j][i] !== '#') continue;
                rect(x + (n * 6 + i) * k + o, y + j * k + o, k, k, col);
            }
        }
    }
}

/* --------------------------------------------------------------- layout */

const COLS = 4, ROWS = 2, SEATS = COLS * ROWS;
const STATION_W = 120, STATION_H = 82, FLOOR_Y = 48, WIN_Y = 212;

function seatGeom(i) {
    const col = i % COLS, row = (i / COLS) | 0;
    return { col, row, sx: col * STATION_W + 20, sy: FLOOR_Y + row * STATION_H };
}

/* Feet positions from the door (under the message window) to a chair. */
function pathTo(i) {
    const { sx, sy, col } = seatGeom(i);
    const gap = (col + (col < COLS / 2 ? 1 : 0)) * STATION_W;
    const aisle = sy + 66;
    return [[W/2, H+22], [W/2, aisle], [gap, aisle], [gap, sy + 28], [sx + 40, sy + 28]];
}

/* Where things sit inside an 80x72 station (offsets from its top-left). */
const SIT_X = 32, SIT_Y = 6;            // seated base box
const CHAIR_X = 30, CHAIR_Y = 2;
const DESK_X = -4, DESK_Y = 30, DESK_W = 88;
const LID_X = 34, LID_Y = 24;

const SEAT_PROPS = [
    [['plant', 15, 25]], [['duck', 56, 29]], [['noodles', 16, 29], ['can', 61, 29]], [['books', 54, 30]],
    [['can', 16, 29]], [['plant', 57, 25]], [['duck', 15, 29]], [['noodles', 56, 29]],
];

/* ------------------------------------------------------------ background */

let BG = null;

function drawWindow(x, y, w, h) {
    rect(x - 3, y - 3, w + 6, h + 6, C.frame);
    rect(x - 3, y - 3, w + 6, 1, C.frameHi);
    rect(x - 2, y - 2, w + 4, h + 4, BASE.K);
    for (let j = 0; j < h; j++) rect(x, y + j, w, 1, j < h / 3 ? C.sky0 : j < (2 * h) / 3 ? C.sky1 : C.sky2);
    // moon
    const mx = x + w - 14, my = y + 6;
    for (let j = -4; j <= 4; j++) for (let i = -4; i <= 4; i++) {
        if (i * i + j * j <= 16 && (i + 2) * (i + 2) + (j - 1) * (j - 1) > 9) px(mx + i, my + j, C.moon);
    }
    // skyline
    const tops = [9, 13, 7, 11, 15, 6, 10, 12, 8, 14, 9, 11];
    let bx = x;
    for (let n = 0; bx < x + w; n++) {
        const bw = 5 + (n * 7) % 5, bh = tops[n % tops.length];
        rect(bx, y + h - bh, Math.min(bw, x + w - bx), bh, n % 2 ? C.city : C.cityB);
        for (let j = y + h - bh + 2; j < y + h - 1; j += 3) for (let i = bx + 1; i < Math.min(bx + bw - 1, x + w); i += 2) {
            if (((i * 31 + j * 17 + n) % 5) === 0) px(i, j, (i + j) % 3 ? C.lit : C.litB);
        }
        bx += bw;
    }
    // mullion and sill
    rect(x + (w >> 1) - 1, y, 2, h, C.frame);
    rect(x, y + (h >> 1) - 1, w, 1, C.frame);
    rect(x - 5, y + h + 3, w + 10, 2, C.sill);
    rect(x - 5, y + h + 5, w + 10, 1, BASE.K);
}

/* The tab: this month's tokens and list-price dollars, Claude and Codex
 * summed. The board and its labels are background; drawTab() chalks up the
 * two values each frame. Its frame mirrors the window's across the wall, and
 * it is as narrow as "TOKENS" plus a 5-character count allows. */
const TAB_X = 397, TAB_Y = 5, TAB_W = 74, TAB_H = 26;

function drawChalkboard(x, y, w, h) {
    rect(x - 2, y - 2, w + 4, h + 4, C.frame);
    rect(x - 2, y - 2, w + 4, 1, C.frameHi);
    rect(x, y, w, h, C.chalk);
    text('TOKENS', x + 3, y + 5, C.chalkDim, 1, null);
    text('COST', x + 3, y + 14, C.chalkDim, 1, null);
}

/* At most 5 characters; rounding through Math.floor so the C port matches. */
function fmtTokens(n) {
    const tenths = (v) => { const t = Math.floor(v + 0.5); return Math.floor(t / 10) + '.' + (t % 10); };
    if (n < 1e3) return String(Math.floor(n));
    if (n < 1e6) return Math.floor(n / 1e3 + 0.5) + 'K';
    if (n < 1e7) return tenths(n / 1e5) + 'M';
    if (n < 1e9) return Math.floor(n / 1e6 + 0.5) + 'M';
    if (n < 1e11) return tenths(n / 1e8) + 'B';
    return Math.floor(n / 1e9 + 0.5) + 'B';
}

function fmtUsd(v) {
    if (v < 999.5) return '$' + Math.floor(v + 0.5);
    if (v < 9999.5) { const t = Math.floor(v / 100 + 0.5); return '$' + Math.floor(t / 10) + '.' + (t % 10) + 'K'; }
    return '$' + Math.floor(v / 1000 + 0.5) + 'K';
}

function drawTab(sc) {
    const u = sc.usage;
    const rows = [[u ? fmtTokens(u.tokens) : '--', C.chalkHi], [u ? fmtUsd(u.usd) : '--', C.chalkGold]];
    rows.forEach(([s, c], r) => text(s, TAB_X + TAB_W - 3 - textWidth(s), TAB_Y + 5 + r * 9, c, 1, null));
}

function renderBackground() {
    // wall
    rect(0, 0, W, 2, C.ceil);
    for (let x = 0; x < W; x++) rect(x, 2, 1, 32, (x >> 3) & 1 ? C.wallB : C.wallA);
    for (let y = 6; y < 32; y += 6) for (let x = ((y / 6) & 1) * 4 + 2; x < W; x += 8) px(x, y, C.wallDot);
    rect(0, 34, W, 1, C.railHi);
    rect(0, 35, W, 2, C.rail);
    rect(0, 37, W, 9, C.wain);
    for (let x = 4; x < W; x += 20) { rect(x, 38, 16, 1, C.wainHi); rect(x, 44, 16, 1, C.wainLo); rect(x, 38, 1, 7, C.wainHi); rect(x + 15, 38, 1, 7, C.wainLo); }
    rect(0, 46, W, 2, C.base);
    drawWindow(10, 6, 58, 24);
    drawChalkboard(TAB_X, TAB_Y, TAB_W, TAB_H);
    // floor: carpet tiles
    for (let y = FLOOR_Y; y < H; y++) for (let x = 0; x < W; x++) {
        const tx = x >> 4, ty = (y - FLOOR_Y) >> 4;
        let c = (tx + ty) & 1 ? C.floorA : C.floorB;
        if ((x & 15) === 0 || ((y - FLOOR_Y) & 15) === 0) c = C.floorLine;
        else if (((x * 7 + y * 13) % 23) === 0) c = C.floorHi;
        px(x, y, c);
    }
    // a runner from the door up the centre aisle
    rect(230, FLOOR_Y + 8, 20, H - FLOOR_Y - 8, C.runner);
    for (let y = FLOOR_Y + 10; y < H; y += 6) rect(233, y, 14, 1, C.runnerHi);
    rect(230, FLOOR_Y + 8, 1, H - FLOOR_Y - 8, C.runnerEdge);
    rect(249, FLOOR_Y + 8, 1, H - FLOOR_Y - 8, C.runnerEdge);
    rect(230, FLOOR_Y + 8, 20, 1, C.runnerEdge);
    // wall shadow on the floor
    darken(0, FLOOR_Y, W, 2, 0.6);
    BG = new Uint8ClampedArray(FB);
}

/* ------------------------------------------------------------ wall life */

const SIGN = 'TOKEN CAFE';
function drawSign(t) {
    const k = 2, w = textWidth(SIGN, k), x0 = (W - w) >> 1, y0 = 8;
    // a tube that dies for a moment now and then
    const cycle = t % 9000, dead = cycle > 7200 && cycle < 7700 && ((cycle / 90) | 0) % 2 === 0 ? 7 : -1;
    for (let pass = 0; pass < 2; pass++) {
        for (let n = 0; n < SIGN.length; n++) {
            const g = GLYPH[SIGN[n]];
            const off = n === dead;
            for (let j = 0; j < 7; j++) for (let i = 0; i < 5; i++) {
                if (g[j][i] !== '#') continue;
                const X = x0 + (n * 6 + i) * k, Y = y0 + j * k;
                if (pass === 0) { if (!off) rect(X - 1, Y - 1, k + 2, k + 2, C.neonGlow); }
                else rect(X, Y, k, k, off ? C.neonOff : (i + j) % 4 === 0 ? C.neonCore : C.neon);
            }
        }
    }
    const sub = 'OPEN 24H', sw = textWidth(sub);
    const sx = (W - sw) >> 1;
    text(sub, sx, y0 + 18, C.cyan, 1, C.cyanGlow);
}

function drawStars(t) {
    const stars = [[14, 9], [22, 14], [31, 8], [40, 12], [17, 18], [46, 7], [27, 19]];
    stars.forEach(([x, y], n) => px(x, y, ((t / 700 + n * 3) | 0) % 5 === 0 ? C.starDim : C.star));
}

function drawClock(cx, cy, date) {
    for (let j = -6; j <= 6; j++) for (let i = -6; i <= 6; i++) {
        const d = i * i + j * j;
        if (d <= 36) px(cx + i, cy + j, d > 25 ? C.clockRim : C.clockFace);
    }
    px(cx, cy - 4, BASE.K); px(cx + 4, cy, BASE.K); px(cx, cy + 4, BASE.K); px(cx - 4, cy, BASE.K);
    const hm = ((date.getHours() % 12) + date.getMinutes() / 60) / 12 * Math.PI * 2;
    const mm = date.getMinutes() / 60 * Math.PI * 2;
    for (let r = 0; r <= 4; r++) px(cx + Math.round(Math.sin(mm) * r), cy - Math.round(Math.cos(mm) * r), BASE.K);
    for (let r = 0; r <= 2; r++) px(cx + Math.round(Math.sin(hm) * r), cy - Math.round(Math.cos(hm) * r), BASE.X);
}

/* ---------------------------------------------------------------- scene */

const WALK_SPEED = 60;      // px per second
const THINK_EVERY = 11000, THINK_FOR = 2600;

function createScene() {
    return { t: 0, seats: Array(SEATS).fill(null), agents: new Map(), particles: [], focus: 0, focusAt: 0, overflow: 0, usage: null };
}

/* This month's totals for the tab, both tools summed: {tokens, usd}. */
function setUsage(sc, usage) { sc.usage = usage; }

function freeSeat(sc) { return sc.seats.indexOf(null); }

/* Reconcile the scene with the host's session list:
 * [{id, name, kind: 'claude'|'codex', state: 'work'|'idle'|'wait', why?: 'perm'|'ask', pose?}] */
function setSessions(sc, list, opts = {}) {
    const seen = new Set();
    let overflow = 0;
    for (const s of list) {
        seen.add(s.id);
        let a = sc.agents.get(s.id);
        if (!a) {
            const seat = freeSeat(sc);
            if (seat < 0) { overflow++; continue; }
            a = { id: s.id, look: lookFor(s.id), seat, since: sc.t, mode: 'seated', path: null, x: 0, y: 0, dist: 0, dir: 'up' };
            sc.seats[seat] = s.id;
            sc.agents.set(s.id, a);
            if (!opts.instant) startWalk(a, pathTo(seat), 'arriving');
        }
        if (a.state !== s.state || a.why !== s.why) a.since = sc.t;
        a.name = s.name; a.kind = s.kind; a.state = s.state; a.why = s.why; a.pose = s.pose;
    }
    for (const a of sc.agents.values()) {
        if (seen.has(a.id) || a.mode === 'leaving') continue;
        const full = pathTo(a.seat);
        if (a.mode === 'arriving') {    // turn round where they stand
            const k = full.length - 1 - a.path.length;
            startWalk(a, [[a.x, a.y], ...full.slice(0, k + 1).reverse()], 'leaving');
        } else startWalk(a, full.slice().reverse(), 'leaving');
    }
    sc.overflow = overflow;
}

function startWalk(a, path, mode) {
    a.mode = mode; a.path = path.slice(1); a.x = path[0][0]; a.y = path[0][1]; a.dist = 0;
}

function tick(sc, dt) {
    sc.t += dt;
    for (const a of [...sc.agents.values()]) {
        if (a.mode === 'seated') continue;
        let step = (WALK_SPEED * dt) / 1000;
        while (step > 0 && a.path.length) {
            const [tx, ty] = a.path[0];
            const dx = tx - a.x, dy = ty - a.y, d = Math.hypot(dx, dy);
            if (d > 0) a.dir = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'right' : 'left') : (dy > 0 ? 'down' : 'up');
            if (d <= step) { a.x = tx; a.y = ty; step -= d; a.dist += d; a.path.shift(); }
            else { a.x += (dx / d) * step; a.y += (dy / d) * step; a.dist += step; step = 0; }
        }
        if (!a.path.length) {
            if (a.mode === 'arriving') { a.mode = 'seated'; a.since = sc.t; }
            else { sc.seats[a.seat] = null; sc.agents.delete(a.id); }
        }
    }
    // particles
    for (const p of sc.particles) { p.age += dt; }
    sc.particles = sc.particles.filter(p => p.age < p.life);
    for (const a of sc.agents.values()) {
        if (a.mode !== 'seated') continue;
        const pose = poseOf(sc, a);
        const { sx, sy } = seatGeom(a.seat);
        a.zAcc = (a.zAcc || 0) + dt;
        if (pose === 'sleep' && a.zAcc > 900) { a.zAcc = 0; sc.particles.push({ kind: 'z', x: sx + 46, y: sy + 12, age: 0, life: 2200 }); }
        if (pose === 'sip' && a.zAcc > 500) {
            a.zAcc = 0;
            const up = ((sc.t + a.look.phase) / 1600 | 0) % 2 === 0;
            sc.particles.push({ kind: 'steam', x: sx + SIT_X + (up ? 10 : 11), y: sy + SIT_Y + (up ? 6 : 11), age: 0, life: 1400, seat: a.seat });
        }
        if (pose === 'type' && a.zAcc > 1300 + (a.look.phase % 700)) {
            a.zAcc = 0;
            const pal = a.kind === 'codex' ? { A: LAPTOP.codex.a } : { A: LAPTOP.claude.a };
            sc.particles.push({ kind: 'key', x: sx + 33 + ((sc.t / 7) | 0) % 12, y: sy + LID_Y - 4, age: 0, life: 1100, glyph: ((sc.t / 13) | 0) % KEYS.length, pal });
        }
    }
    // the message window cycles through everyone who is waiting
    const waiting = waitingList(sc);
    if (sc.t - sc.focusAt > 3500) { sc.focusAt = sc.t; sc.focus++; }
    if (sc.focus >= waiting.length) sc.focus = 0;
}

function waitingList(sc) {
    return [...sc.agents.values()].filter(a => a.mode === 'seated' && a.state === 'wait').sort((a, b) => a.since - b.since);
}

/* Which seated pose an agent is in right now. */
function poseOf(sc, a) {
    if (a.state === 'wait') return 'raise';
    if (a.state === 'work') {
        const ph = (sc.t + a.look.phase) % THINK_EVERY;
        return a.pose === 'think' || ph < THINK_FOR ? 'think' : 'type';
    }
    if (a.pose) return a.pose;
    // idle: the longer the human leaves them, the deeper the slack
    const idle = sc.t - a.since;
    if (idle > 10 * 60000) return 'sleep';
    if (idle > 2 * 60000) return 'sip';
    return a.look.variant === 2 ? 'sip' : 'stretch';
}

/* --------------------------------------------------------------- render */

function drawChair(i) {
    const { sx, sy } = seatGeom(i);
    const [a, b] = CHAIRS[i % CHAIRS.length];
    blit(CHAIR, sx + CHAIR_X, sy + CHAIR_Y, { A: q(a), a: q(b) });
}

function drawDesk(sc, i) {
    const { sx, sy } = seatGeom(i);
    const x = sx + DESK_X, y = sy + DESK_Y, w = DESK_W;
    darken(x + 2, y + 21, w - 2, 3, 0.55);          // floor shadow
    rect(x, y, w, 9, BASE.K);                        // top surface
    rect(x + 1, y + 1, w - 2, 7, C.deskTop);
    rect(x + 1, y + 1, w - 2, 1, C.deskHi);
    for (let n = 0; n < 3; n++) rect(x + 7 + n * 16, y + 3 + (n & 1) * 2, 8, 1, C.deskGrain);
    rect(x + 1, y + 7, w - 2, 1, C.deskEdge);
    rect(x, y + 8, w, 13, BASE.K);                   // front panel
    rect(x + 1, y + 8, w - 2, 12, C.deskFront);
    rect(x + 1, y + 18, w - 2, 2, C.deskFrontLo);
    for (const [name, ox, oy] of SEAT_PROPS[i]) blit(PROPS[name], sx + ox, sy + oy);
    const id = sc.seats[i];
    const a = id && sc.agents.get(id);
    if (a && a.mode === 'seated') {
        if (poseOf(sc, a) === 'sleep') drawClosedLaptop(a, sx + LID_X - 1, sy + DESK_Y + 2);
        else drawLaptop(sc, a, sx + LID_X, sy + LID_Y);
    }
    if (a) drawPlate(sc, a, sx + 40, y + 9);
    else drawPlate(sc, null, sx + 40, y + 9);
}

function drawClosedLaptop(a, x, y) {
    const pal = LAPTOP[a.kind] || LAPTOP.claude;
    rect(x, y, 14, 4, BASE.K);
    rect(x + 1, y + 1, 12, 1, pal.L);
    rect(x + 1, y + 2, 12, 1, pal.l);
}

function drawLaptop(sc, a, x, y) {
    const pal = LAPTOP[a.kind] || LAPTOP.claude;
    blit(LID, x, y, pal);
    rect(x - 1, y + 11, 15, 2, BASE.K);
    rect(x, y + 11, 13, 1, pal.l);
    let logo;
    if (a.kind === 'codex') logo = PROMPT[a.state === 'work' ? ((sc.t / 450) | 0) % 2 : 1];
    else if (a.state === 'work') { const seq = [0, 1, 2, 3, 2, 1]; logo = SPARK[seq[((sc.t / 130) | 0) % seq.length]]; }
    else logo = SPARK[3];
    let lp = pal;
    if (a.state === 'wait' && ((sc.t / 300) | 0) % 2) lp = Object.assign({}, pal, { A: BASE.Y, a: BASE.W });
    blit(logo, x + 3, y + 2, lp);
}

function drawPlate(sc, a, cx, y) {
    if (!a) {   // an empty seat keeps a blank brass plate
        rect(cx - 12, y + 2, 24, 7, BASE.K);
        rect(cx - 11, y + 3, 22, 5, C.deskFrontLo);
        return;
    }
    const name = (a.name || '').toUpperCase().slice(0, 8);
    const w = textWidth(name) + 6, x = cx - (w >> 1);
    const blink = a.state === 'wait' && ((sc.t / 400) | 0) % 2 === 0;
    rect(x, y, w, 11, BASE.K);
    rect(x + 1, y + 1, w - 2, 9, blink ? BASE.Y : a.kind === 'codex' ? C.plateCodex : C.plateClaude);
    rect(x + 1, y + 9, w - 2, 1, C.plateLo);
    text(name, x + 3, y + 2, a.state === 'idle' ? q('#7a7068') : BASE.K, 1, null);
}

function poseSet(a) { return POSES[a.look.style] || POSE; }

function drawSeated(sc, a) {
    const { sx, sy } = seatGeom(a.seat);
    const P = poseSet(a);
    const t = sc.t + a.look.phase;
    let spr;
    switch (poseOf(sc, a)) {
        case 'type': spr = P.type[((t / 170) | 0) % 2]; break;
        case 'think': spr = P.think; break;
        case 'raise': spr = P.raise[((t / 320) | 0) % 2]; break;
        case 'stretch': spr = P.stretch; break;
        case 'sip': spr = P.sip[((t / 1600) | 0) % 2]; break;
        case 'sleep': spr = P.sleep[((t / 1100) | 0) % 2]; break;
        default: spr = P.type[0];
    }
    blit(spr, sx + SIT_X - BX, sy + SIT_Y - BY, a.look);
}

function drawWalker(sc, a) {
    const P = poseSet(a);
    const f = ((a.dist / 7) | 0) % 4;
    let spr, flip = false;
    const pick = (stand, walk) => (f % 2 === 0 ? stand : walk[f === 1 ? 0 : 1]);
    if (a.dir === 'up') spr = pick(P.standU, P.walkU);
    else if (a.dir === 'down') spr = pick(P.standD, P.walkD);
    else { spr = f % 2 === 0 ? P.standS : P.walkS[0]; flip = a.dir === 'left'; }
    const x = Math.round(a.x) - 8, y = Math.round(a.y) - 24;
    blit(spr, x - BX, y - BY + (f % 2 ? -1 : 0), a.look, flip);
}

function drawOverlays(sc) {
    const waiting = waitingList(sc);
    const focused = waiting[sc.focus];
    for (const a of sc.agents.values()) {
        if (a.mode !== 'seated') continue;
        const { sx, sy } = seatGeom(a.seat);
        const pose = poseOf(sc, a);
        if (pose === 'raise') {
            const bob = ((sc.t / 260) | 0) % 2;
            blit(a.why === 'ask' ? BUBBLE.ask : BUBBLE.bang, sx + 29, sy + SIT_Y - 14 - bob);
        } else if (pose === 'think') {
            blit(BUBBLE.think, sx + 45, sy + SIT_Y - 11);
        }
        if (a === focused) {
            const nudge = ((sc.t / 220) | 0) % 2;
            blit(CURSOR, sx + 16 + nudge, sy + SIT_Y + 6);
        }
    }
    for (const p of sc.particles) {
        const k = p.age / p.life;
        if (p.kind === 'z') {
            const x = Math.round(p.x + k * 10 + Math.sin(k * 6) * 2), y = Math.round(p.y - k * 20);
            blit(k < 0.45 ? ZSMALL : ZBIG, x, y);
        } else if (p.kind === 'steam') {
            const x = Math.round(p.x + Math.sin(k * 7 + p.seat) * 1.5), y = Math.round(p.y - k * 9);
            px(x, y, k < 0.5 ? BASE.W : BASE.w);
        } else if (p.kind === 'key') {
            const x = Math.round(p.x), y = Math.round(p.y - k * 10);
            blit(KEYS[p.glyph], x, y, p.pal);
        }
    }
}

/* FF-style window: dark rim, light double border with clipped corners, banded blue fill. */
function drawWindowBox(x, y, w, h) {
    rect(x + 2, y, w - 4, h, C.winOuter);
    rect(x, y + 2, w, h - 4, C.winOuter);
    rect(x + 1, y + 1, w - 2, h - 2, C.winOuter);
    rect(x + 2, y + 1, w - 4, h - 2, C.winBorder);
    rect(x + 1, y + 2, w - 2, h - 4, C.winBorder);
    rect(x + 3, y + 3, w - 6, h - 6, C.winBorderLo);
    const ih = h - 8, bands = 8;
    for (let j = 0; j < ih; j++) {
        const k = Math.floor((j / ih) * bands) / (bands - 1);
        const c = q3(C.win0[0] + (C.win1[0] - C.win0[0]) * k, C.win0[1] + (C.win1[1] - C.win0[1]) * k, C.win0[2] + (C.win1[2] - C.win0[2]) * k);
        rect(x + 4, y + 4 + j, w - 8, 1, c);
    }
}

const WHY_TEXT = { perm: 'NEEDS YOUR OK!', ask: 'HAS A QUESTION' };

function drawMessage(sc, date) {
    const x = 0, y = WIN_Y, w = W, h = H - WIN_Y;
    drawWindowBox(x, y, w, h);
    const waiting = waitingList(sc);
    const a = waiting[sc.focus];
    if (a) {
        // portrait: the agent's head at 2x in an inset frame
        const fx = x + 8, fy = y + 7, fs = 34;
        rect(fx, fy, fs, fs, C.winBorderLo);
        rect(fx + 1, fy + 1, fs - 2, fs - 2, C.win1);
        const [cA] = CHAIRS[a.seat % CHAIRS.length];
        rect(fx + 1, fy + 1 + 18, fs - 2, fs - 20, q(cA));
        blitScaled(HEAD.shout, fx + 3, fy + 5, a.look, 2, [fx + 1, fy + 1, fx + fs - 1, fy + fs - 1]);
        const tx = fx + fs + 8;
        text((a.name || '').toUpperCase().slice(0, 12), tx, y + 9, C.gold, 2);
        text(WHY_TEXT[a.why] || 'NEEDS YOU', tx, y + 27, C.text, 2);
        if (waiting.length > 1) {
            const s = `${sc.focus + 1}/${waiting.length}`;
            text(s, x + w - 10 - textWidth(s), y + 9, C.textDim, 1);
        }
        if (((sc.t / 400) | 0) % 2 === 0) {
            const ax = x + w - 16, ay = y + h - 13;
            for (let j = 0; j < 4; j++) rect(ax + j, ay + j, 7 - 2 * j, 1, C.text);
        }
        return;
    }
    // nobody waiting: party status
    const seated = [...sc.agents.values()].filter(v => v.mode === 'seated');
    const working = seated.filter(v => v.state === 'work').length;
    const idle = seated.filter(v => v.state === 'idle').length;
    let cx = x + 12;
    const stat = (icon, pal, n) => {
        blitScaled(icon, cx, y + 9, pal, 2);
        cx += icon.w * 2 + 4;
        text(String(n), cx, y + 10, C.text, 2);
        cx += textWidth(String(n), 2) + 14;
    };
    stat(ICON.laptop, { A: q('#9ad0ff'), a: q('#3a70c0') }, working);
    stat(ICON.cup, null, idle);
    const hh = String(date.getHours()).padStart(2, '0'), mm = String(date.getMinutes()).padStart(2, '0');
    const colon = ((sc.t / 500) | 0) % 2 ? ':' : ' ';
    const clock = hh + colon + mm;
    text(clock, x + w - 12 - textWidth(clock, 2), y + 10, C.gold, 2);
    let line;
    if (!seated.length) line = 'THE CAFE IS EMPTY. NO AGENTS ONLINE.';
    else if (!idle) line = working === 1 ? 'ONE AGENT IS HARD AT WORK.' : `ALL ${working} AGENTS ARE HARD AT WORK.`;
    else if (!working) line = 'EVERYONE IS ON A BREAK. YOUR MOVE.';
    else line = `${idle} ON A BREAK, ${working} AT WORK.`;
    if (sc.overflow) line = `+${sc.overflow} WAITING OUTSIDE. ` + line;
    text(line, x + 12, y + 31, C.textDim, 1);
}

function render(sc, out, date = new Date()) {
    FB = out;
    if (!BG) renderBackground();
    FB.set(BG);
    drawStars(sc.t);
    drawSign(sc.t);
    drawClock(86, 17, date);
    drawTab(sc);
    // depth-sorted scene
    const items = [];
    for (let i = 0; i < SEATS; i++) {
        const { sy } = seatGeom(i);
        items.push([sy + 27, () => drawChair(i)]);
        items.push([sy + 51, () => drawDesk(sc, i)]);
    }
    for (const a of sc.agents.values()) {
        if (a.mode === 'seated') items.push([seatGeom(a.seat).sy + 28, () => drawSeated(sc, a)]);
        else items.push([a.y, () => drawWalker(sc, a)]);
    }
    items.sort((p, r) => p[0] - r[0]);
    for (const [, f] of items) f();
    drawOverlays(sc);
    drawMessage(sc, date);
}

function hit(sc, x, y) {
    if (y >= WIN_Y || y < FLOOR_Y - 8) return null;
    const col = Math.floor(x / STATION_W), row = Math.floor((y - FLOOR_Y + 8) / STATION_H);
    if (col < 0 || col >= COLS || row < 0 || row >= ROWS) return null;
    return sc.seats[row * COLS + col];
}

/* Debug: every character pose on a plain ground, for sprite review. */
let SHEET_STYLE = 'short';
function sheet(out, style) {
    SHEET_STYLE = style || 'short';
    FB = out;
    rect(0, 0, W, H, C.floorB);
    const look = Object.assign(lookFor('3c10323c'), { style: SHEET_STYLE });
    const list = [];
    for (const [k, v] of Object.entries(POSES[SHEET_STYLE])) (Array.isArray(v) ? v : [v]).forEach((s, n) => list.push([k + (Array.isArray(v) ? n : ''), s]));
    list.forEach(([name, s], n) => {
        const x = (n % 8) * 40 + 4, y = ((n / 8) | 0) * 46 + 4;
        rect(x, y, CW, CH, C.floorA);
        blit(s, x, y, look);
        text(name.slice(0, 6), x, y + CH + 2, C.text, 1, null);
    });
}

/* Asset inventory, for the budget table. */
function inventory() {
    const all = [];
    const add = (s) => all.push(s);
    Object.values(HEAD).forEach(add);
    Object.values(POSE).forEach(v => (Array.isArray(v) ? v.forEach(add) : add(v)));
    [CHAIR, LID, ZSMALL, ZBIG, CURSOR].forEach(add);
    SPARK.forEach(add); PROMPT.forEach(add);
    Object.values(PROPS).forEach(add); Object.values(BUBBLE).forEach(add); Object.values(ICON).forEach(add);
    return all.map(s => ({ name: s.name, w: s.w, h: s.h, bytes4bpp: Math.ceil((s.w * s.h) / 2) }));
}

/* Everything the firmware needs, for tools/gen_assets.mjs: palettes, sprites,
 * the font and the static room (background + empty message window). */
function exportAssets() {
    FB = new Uint8ClampedArray(W * H * 4);
    BG = null;
    renderBackground();
    drawWindowBox(0, WIN_Y, W, H - WIN_Y);
    const bg = new Uint8ClampedArray(FB);
    BG = null;      // render() rebuilds its own background without the window
    return {
        W, H, WIN_Y, BASE, C, LAPTOP, HAIR_STYLES,
        SKINS: SKINS.map(p => p.map(q)), HAIRS: HAIRS.map(p => p.map(q)), SHIRTS: SHIRTS.map(p => p.map(q)),
        PANTS: PANTS.map(q), CHAIRS: CHAIRS.map(p => p.map(q)),
        LOOK_V: q('#f4f2ec'), LOOK_F: q('#3a2420'), IDLE_TEXT: q('#7a7068'),
        ICON_LAPTOP_A: q('#9ad0ff'), ICON_LAPTOP_a: q('#3a70c0'),
        HEAD, POSES, CHAIR, LID, SPARK, PROMPT, PROPS, BUBBLE, ZSMALL, ZBIG, KEYS, CURSOR, ICON, GLYPH, bg,
    };
}

const api = { exportAssets, POSES, sheet, W, H, SEATS, createScene, setSessions, setUsage, tick, render, hit, poseOf, seatGeom, inventory, POSE, HEAD };
if (typeof module !== 'undefined' && module.exports) module.exports = api;
else root.TokenCafe = api;
})(typeof window !== 'undefined' ? window : globalThis);
