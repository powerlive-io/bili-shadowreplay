import { box, diamond, label, region, text, type Diagram } from './drawing';

// Both rulers show 14 seconds at 48 units per second. Their origins differ:
// material 12s and project 30s line up for the unedited placement.
type Span = [start: number, end: number];
const timeX = (second: number, origin: number) => 232 + (second - origin) * 48;

function ruler(y: number, origin: number) {
  const ticks = Array.from({ length: 8 }, (_, i) => {
    const second = origin + i * 2, x = timeX(second, origin);
    return `<path d="M ${x} ${y - 4} v 8" stroke="var(--stroke)"/>`
      + text(x, y - 10, String(second), 'fig-detail', 'middle');
  });
  return `<path d="M ${timeX(origin, origin)} ${y} h ${14 * 48}" stroke="var(--stroke)"/>` + ticks.join('');
}

function timeStrip(y: number, name: string, origin: number, clip: Span, cue: Span, focus = false) {
  const band = ([start, end]: Span, fill: string) =>
    `<rect x="${timeX(start, origin)}" y="${y}" width="${(end - start) * 48}" height="32" rx="4" fill="var(--${fill})" stroke="var(--stroke)"/>`;
  return text(40, y + 12, name, 'fig-name') + text(40, y + 32, `字幕 ${cue[0]}–${cue[1]} 秒`, 'fig-detail')
    + band(clip, 'surface') + `<g${focus ? ' class="fig-focus"' : ''}>` + band(cue, focus ? 'clay' : 'storage')
    + text(timeX((cue[0] + cue[1]) / 2, origin), y + 21, '字幕', 'fig-detail', 'middle') + '</g>';
}

export const mediaWorkflows: Diagram[] = [
  {
    id: 'whole-recording-encoding', rows: [0, 400],
    title: '生成完整录播时，哪些片段需要重新编码？',
    description: '手动合成时，重新编码和压制弹幕都关闭、且各段显示尺寸一致，才直接复制视频流。',
    draw: w => [
      region(24, 20, 464, 360, '按勾选顺序合成'), region(560, 20, 376, 360, '本次处理方式'),
      w([[444, 100], [592, 100]]), label(520, 76, '任一开启'),
      w([[264, 136], [264, 192]]), label(302, 156, '都未开启', 'panel'),
      w([[444, 228], [592, 228]]), label(520, 204, '一致'),
      w([[264, 264], [264, 332], [592, 332]]), label(296, 288, '不一致', 'panel'),
      diamond(264, 100, 360, 72, '重新编码或压制弹幕？'), diamond(264, 228, 360, 72, '各段显示尺寸一致？'),
      box(592, 68, 312, '按所选参数编码', '只有一段也会编码'),
      box(592, 196, 312, '直接复制视频流', '不改变画面尺寸'),
      box(592, 300, 312, '按默认参数自动编码', '取像素面积最大的片段尺寸作为画布', 'focus'),
    ],
  },
  {
    id: 'subtitle-cache-and-overwrite', rows: [0, 432],
    title: 'AI 字幕先选片段，再决定是否重新识别素材',
    description: '「覆盖」决定哪些片段可以改；「忽略缓存」只决定这些片段所用的素材要不要重新识别。',
    draw: w => [
      region(24, 20, 300, 284, '先看每个片段'), region(356, 20, 580, 392, '再按素材取得字幕'),
      w([[176, 152], [176, 220]]), label(196, 176, '是', 'panel'),
      w([[308, 112], [468, 112]]), label(340, 88, '否'),
      w([[620, 152], [620, 180], [496, 180], [496, 220]]), label(558, 156, '有缓存且未忽略', 'panel'),
      w([[772, 112], [796, 112], [796, 220]]), label(842, 156, '缺失或忽略', 'panel'),
      w([[496, 284], [496, 312], [584, 312], [584, 332]]),
      w([[796, 284], [796, 312], [712, 312], [712, 332]]),
      diamond(176, 112, 264, 80, '已有字幕且不覆盖？'),
      box(44, 220, 264, '保留此片段的字幕', '若同一素材的片段全跳过，就不识别'),
      diamond(620, 112, 304, 80, '能复用素材 SRT？'),
      box(376, 220, 240, '读取素材旁的 SRT', '不调用识别服务', 'store'),
      box(676, 220, 240, '重新识别并缓存 SRT', '本次同一素材只识别一次'),
      box(456, 332, 384, '按片段当前范围回填', '写入前再确认该片段仍需字幕', 'focus'),
    ],
  },
  {
    id: 'subtitle-time-mapping', rows: [0, 432],
    title: '素材的第 14 秒，到了工程里是第几秒？',
    description: '同一条字幕在素材中占 14–16 秒；前移片段和裁掉片头分别与原位置比较，素材字幕本身不变。',
    draw: () => [
      region(24, 20, 912, 120, '素材时间（秒）'), ruler(80, 6),
      timeStrip(92, '取素材 12–20 秒', 6, [12, 20], [14, 16]),
      region(24, 160, 912, 252, '工程时间（秒）'), ruler(224, 24),
      timeStrip(240, '原位置：从 30 秒开始', 24, [30, 38], [32, 34], true),
      timeStrip(300, '前移到工程 24 秒', 24, [24, 32], [26, 28]),
      timeStrip(360, '只把素材入点裁到 15 秒', 24, [30, 35], [30, 31]),
    ],
  },
];
