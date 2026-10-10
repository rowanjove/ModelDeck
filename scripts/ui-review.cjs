// Isolated browser fixtures; no provider requests or application data writes.
const runtime = process.env.MODELDECK_PLAYWRIGHT || require('node:path').join(require('node:os').homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const { chromium } = require(runtime);
const fs = require('node:fs');
const path = require('node:path');
const out = path.resolve('release-dist/ui-review');
fs.mkdirSync(out, { recursive: true });

async function fixture(page, mode = 'normal') {
  await page.addInitScript(({ mode }) => {
    const now = '2026-10-02T08:00:00Z';
    const providers = [{ id: 'p', name: 'Studio API', type: 'openai-compatible', baseUrl: 'https://example.invalid/v1', hasCredential: true, headers: {}, timeoutMs: 120000, enabled: true, createdAt: now, updatedAt: now }];
    const params = [{ key: 'temperature', label: 'Temperature', type: 'range', min: 0, max: 2, step: 0.1, default: 0.7 }, { key: 'seed', label: 'Seed', type: 'number', advanced: true }, { key: 'quality', label: 'Quality', type: 'select', options: [{ label: 'Standard', value: 'standard' }, { label: 'High', value: 'high' }], default: 'standard' }];
    const models = ['text', 'image', 'video', 'audio'].map(modality => ({ id: modality, name: `${modality} / Studio`, providerId: 'p', modelId: `${modality}-model`, modality, capabilities: { generate: true, vision: true, reasoning: true, textToVideo: true, firstFrame: true, lastFrame: true, referenceImages: true, referenceVideos: true, referenceAudio: true, textToSpeech: true }, parameters: params, constraints: [], transport: {}, enabled: true, createdAt: now, updatedAt: now }));
    const history = ['completed', 'running', 'queued', 'failed', 'cancelled', 'interrupted'].map((status, i) => ({ id: `h${i}`, modality: 'text', providerId: 'p', providerName: 'Studio API', modelId: 'text', modelName: 'text / Studio', status, promptPreview: '分析设计方案，整理关键约束与下一步。'.repeat(5), createdAt: now, completedAt: now }));
    window.__uiCommands = [];
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      unregisterCallback: () => {},
      invoke: async (cmd, args) => {
        window.__uiCommands.push(cmd);
        if (mode === 'loading' && ['models_list', 'providers_list'].includes(cmd)) return new Promise(() => {});
        if (mode === 'offline' && ['models_list', 'providers_list'].includes(cmd)) throw { code: 'NetworkError', message: '服务暂时不可用，请检查网络连接后重试。' };
        if (cmd === 'providers_list') return mode === 'empty' ? [] : providers;
        if (cmd === 'models_list') return mode === 'empty' ? [] : models;
        if (cmd === 'history_list') return { items: mode === 'empty' ? [] : history, total: mode === 'empty' ? 0 : 6, offset: 0, limit: 30 };
        if (cmd === 'history_get') return { ...history.find(h => h.id === args.id), kind: 'generate', modelApiId: 'text-model', prompt: '设计任务输入', parameters: {}, images: [], rawRequest: {}, rawResponse: {}, output: '检查结果\n' + '这是工作区预览，用于检查较长的结果排版。'.repeat(80), assets: [], latencyMs: 1234, usage: { totalTokens: 240 }, statusCode: 200 };
        if (cmd === 'conversations_list') return [{ id: 'c', title: '方案审阅', modelDefinitionId: 'text', systemPrompt: '', contextStrategy: 'all', contextLimit: 8192, createdAt: now, updatedAt: now }];
        if (cmd === 'conversations_get') return { conversation: { id: 'c', title: '方案审阅', modelDefinitionId: 'text', systemPrompt: '', contextStrategy: 'all', contextLimit: 8192, createdAt: now, updatedAt: now }, messages: [{ id: 'u', role: 'user', text: '审阅这份设计方案，梳理约束。', tokenCount: 15, createdAt: now }, { id: 'a', role: 'assistant', text: '建议先确认输入和输出边界，再逐步验证主工作区布局。\n' + '保留必要信息，减少重复层级。'.repeat(20), tokenCount: 100, createdAt: now }] };
        if (cmd.includes('token')) return 120;
        if (cmd.includes('listen')) return 1;
        return [];
      },
      convertFileSrc: () => '',
    };
  }, { mode });
}

(async () => {
  const browser = await chromium.launch({ headless: true });
  const report = [];
  for (const [width, height, scale, label] of [[1440, 900, 1, 'desktop'], [1152, 720, 1.25, '125'], [960, 600, 1.5, '150'], [720, 540, 1, 'small'], [1920, 1080, 1, 'large']].filter(size => !process.env.UI_REVIEW_SIZE || size[3] === process.env.UI_REVIEW_SIZE)) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale });
    const page = await context.newPage();
    await fixture(page);
    await page.goto('http://127.0.0.1:1420');
    await page.locator('.workspace').waitFor();
    for (const name of ['文本', '图像', '视频', '音频', '工作流', '历史', '模型', '设置']) {
      await page.locator('.nav-item').filter({ hasText: name }).click();
      await page.waitForFunction(() => !document.querySelector('main').innerText.includes('正在加载模块'));
      await page.waitForTimeout(350);
      if (name === '模型') await page.locator('.catalog-list:visible li button').first().click();
      if (name === '设置') await page.locator('.catalog-list:visible li button').first().click();
      const metrics = await page.evaluate(() => ({ errors: document.querySelector('main').innerText.includes('模块发生错误'), overflow: [...document.querySelectorAll('.main, .workspace, .topbar, .settings-header')].filter(e => e.getClientRects().length && e.scrollWidth > e.clientWidth + 2).map(e => e.className) }));
      report.push({ label, page: name, ...metrics });
      await page.screenshot({ path: path.join(out, `${label}-${name}.png`) });
    }
    await page.locator('.nav-item').filter({ hasText: '文本' }).click();
    await page.getByRole('button', { name: '对话', exact: true }).click();
    await page.waitForTimeout(200);
    await page.getByRole('button', { name: /方案审阅/ }).click();
    await page.waitForTimeout(200);
    await page.screenshot({ path: path.join(out, `${label}-chat.png`) });
    report.push({ label, page: 'Chat', rail: await page.locator('.chat-rail').isVisible() });
    if (label === 'desktop' || label === 'small') {
      await page.getByRole('checkbox', { name: 'Agent', exact: true }).check();
      await page.screenshot({ path: path.join(out, `${label}-agent.png`) });
      await page.getByRole('button', { name: '工具权限', exact: true }).click();
      await page.getByRole('dialog', { name: '工具权限' }).waitFor();
      await page.screenshot({ path: path.join(out, `${label}-permissions.png`) });
      for (let i = 0; i < 6; i++) {
        await page.keyboard.press('Tab');
        if (!await page.evaluate(() => !!document.activeElement.closest('dialog'))) throw new Error('Dialog focus escaped');
      }
      await page.keyboard.press('Escape');
      if (await page.getByRole('dialog').count()) throw new Error('Escape did not close the permissions dialog');
      await page.locator('.nav-item').filter({ hasText: '音频' }).click();
      await page.getByRole('button', { name: /^声音库 \(/ }).click();
      await page.getByRole('dialog', { name: '声音库' }).waitFor();
      await page.getByRole('button', { name: /新建声音/ }).click();
      await page.screenshot({ path: path.join(out, `${label}-voices.png`) });
      await page.keyboard.press('Escape');
      await page.locator('.nav-item').filter({ hasText: '图像' }).click();
      await page.locator('#image-prompt').fill('界面检查草稿');
      await page.evaluate(async () => {
        const { useTextDraft } = await import('/src/store.ts');
        useTextDraft.getState().patch({ prompt: '隐藏页面不应执行' });
        window.__uiCommands.length = 0;
      });
      await page.locator('#image-prompt').press('Control+Enter');
      const commands = await page.evaluate(() => window.__uiCommands);
      if (commands.some(cmd => cmd === 'generate_text')) throw new Error('Hidden text workspace received Ctrl+Enter');
      report.push({ label, interaction: 'visible-workspace-shortcut', commands });
      for (const eventType of ['queued', 'progress', 'failed', 'cancelled', 'completed']) {
        await page.evaluate(async eventType => {
          const { useImageDraft } = await import('/src/store.ts');
          const { useTaskQueueStore } = await import('/src/lib/taskQueue.ts');
          useImageDraft.getState().patch({ activeTaskId: 'fixture-task' });
          useTaskQueueStore.getState().upsertTask({ taskId: 'fixture-task', eventType, status: eventType === 'progress' ? 'running' : eventType, modality: 'image', progress: eventType === 'progress' ? 0.42 : null, message: eventType === 'queued' ? '等待服务处理' : '界面状态检查', error: eventType === 'failed' ? '服务暂时不可用，请稍后重试。' : null, outputAsset: eventType === 'completed' ? { images: [], latencyMs: 1234, status: 200 } : undefined, timestamp: '2026-10-02' });
        }, eventType);
        await page.waitForTimeout(100);
        await page.screenshot({ path: path.join(out, `${label}-state-${eventType}.png`) });
      }
      const previous = await page.locator('#image-prompt').inputValue();
      const scroll = await page.evaluate(() => { const element = [...document.querySelectorAll('.composer')].find(e => e.getClientRects().length); if (element) { element.scrollTop = 60; return element.scrollTop; } return 0; });
      await page.locator('.nav-item').filter({ hasText: '历史' }).click();
      await page.locator('.nav-item').filter({ hasText: '图像' }).click();
      if (await page.locator('#image-prompt').inputValue() !== previous) throw new Error('Page switch lost the draft');
      const restoredScroll = await page.evaluate(() => [...document.querySelectorAll('.composer')].find(e => e.getClientRects().length)?.scrollTop ?? 0);
      if (restoredScroll !== scroll) throw new Error('Page switch lost scroll position');
      report.push({ label, interaction: 'dialog-focus-escape-draft-scroll', passed: true });
    }
    await context.close();
  }
  for (const mode of ['empty', 'offline', 'loading']) {
    const page = await browser.newPage({ viewport: { width: 960, height: 600 } });
    await fixture(page, mode);
    await page.goto('http://127.0.0.1:1420');
    await page.waitForTimeout(mode === 'offline' ? 8000 : 600);
    await page.screenshot({ path: path.join(out, `state-${mode}.png`) });
    report.push({ mode, text: await page.locator('main').innerText() });
    await page.close();
  }
  fs.writeFileSync(path.join(out, process.env.UI_REVIEW_SIZE ? `report-${process.env.UI_REVIEW_SIZE}.json` : 'report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report));
  await browser.close();
  if (report.some(row => row.errors || row.overflow?.length)) process.exitCode = 1;
})().catch(e => { console.error(e); process.exit(1); });
