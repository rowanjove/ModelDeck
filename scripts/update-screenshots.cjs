// Update high-resolution screenshots directly to docs/screenshots
const runtime = process.env.MODELDECK_PLAYWRIGHT || require('node:path').join(require('node:os').homedir(), '.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const { chromium } = require(runtime);
const fs = require('node:fs');
const path = require('node:path');

const outDir = path.resolve('docs/screenshots');
fs.mkdirSync(outDir, { recursive: true });

async function fixture(page) {
  await page.addInitScript(() => {
    const now = '2026-10-11T08:00:00Z';
    const providers = [
      { id: 'p', name: 'Studio API', type: 'openai-compatible', baseUrl: 'https://api.deepseek.com', hasCredential: true, headers: {}, timeoutMs: 120000, enabled: true, createdAt: now, updatedAt: now }
    ];
    const params = [
      { key: 'temperature', label: 'Temperature', type: 'range', min: 0, max: 2, step: 0.1, default: 0.7 },
      { key: 'seed', label: 'Seed', type: 'number', advanced: true },
      { key: 'quality', label: 'Quality', type: 'select', options: [{ label: 'Standard', value: 'standard' }, { label: 'High', value: 'high' }], default: 'standard' }
    ];
    const models = ['text', 'image', 'video', 'audio'].map(modality => ({
      id: modality,
      name: `${modality === 'text' ? 'DeepSeek-V3' : modality === 'image' ? 'Flux.1 Schnell' : modality === 'video' ? 'Kling 1.5' : 'CosyVoice 2'} / Studio`,
      providerId: 'p',
      modelId: `${modality}-model`,
      modality,
      capabilities: {
        generate: true,
        vision: true,
        reasoning: true,
        textToVideo: true,
        firstFrame: true,
        lastFrame: true,
        referenceImages: true,
        referenceVideos: true,
        referenceAudio: true,
        textToSpeech: true
      },
      parameters: params,
      constraints: [],
      transport: {},
      enabled: true,
      createdAt: now,
      updatedAt: now
    }));
    const history = ['completed', 'running', 'queued', 'failed', 'cancelled', 'interrupted'].map((status, i) => ({
      id: `h${i}`,
      modality: 'text',
      providerId: 'p',
      providerName: 'Studio API',
      modelId: 'text',
      modelName: 'DeepSeek-V3 / Studio',
      status,
      promptPreview: '分析现代桌面应用中的 Local-First 架构与安全凭据设计。'.repeat(3),
      createdAt: now,
      completedAt: now
    }));

    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      unregisterCallback: () => {},
      invoke: async (cmd, args) => {
        if (cmd === 'providers_list') return providers;
        if (cmd === 'models_list') return models;
        if (cmd === 'history_list') return { items: history, total: history.length, offset: 0, limit: 30 };
        if (cmd === 'history_get') return {
          ...history[0],
          kind: 'generate',
          modelApiId: 'text-model',
          prompt: '设计任务输入',
          parameters: {},
          images: [],
          rawRequest: {},
          rawResponse: {},
          output: 'ModelDeck 基于本地优先原则设计，API Key 托管于 Windows Credential Manager。',
          assets: [],
          latencyMs: 842,
          usage: { totalTokens: 180 },
          statusCode: 200
        };
        if (cmd === 'conversations_list') return [{ id: 'c', title: '桌面端架构设计与约束分析', modelDefinitionId: 'text', systemPrompt: '', contextStrategy: 'all', contextLimit: 8192, createdAt: now, updatedAt: now }];
        if (cmd === 'conversations_get') return {
          conversation: { id: 'c', title: '桌面端架构设计与约束分析', modelDefinitionId: 'text', systemPrompt: '', contextStrategy: 'all', contextLimit: 8192, createdAt: now, updatedAt: now },
          messages: [
            { id: 'u', role: 'user', text: '请梳理 ModelDeck 的本地安全策略与 Rust 后端的核心优势。', tokenCount: 20, createdAt: now },
            { id: 'a', role: 'assistant', text: '### ModelDeck 核心架构优势\n\n1. **Windows 凭据保护 (DPAPI)**：敏感凭据隔离在系统原生凭据安全库中。\n2. **工业级持久后台队列**：支持断电恢复与 CAS 状态自愈。\n3. **轻量无感知启动**：不捆绑重型 Python 运行时，极低内存消耗。', tokenCount: 120, createdAt: now }
          ]
        };
        if (cmd === 'search_unified') return {
          total: 2,
          page: 1,
          pageSize: 20,
          items: [
            { id: 's1', category: 'conversation', title: '桌面端架构设计与约束分析', snippet: '请梳理 ModelDeck 的本地安全策略与 Rust 后端的核心优势...', modality: 'text', targetRoute: 'text', timestamp: now },
            { id: 's2', category: 'model', title: 'DeepSeek-V3 / Studio', snippet: '高性能对话模型，已配置 Studio API 服务端点', modality: 'text', targetRoute: 'models', timestamp: now }
          ]
        };
        if (cmd.includes('token')) return 120;
        if (cmd.includes('listen')) return 1;
        return [];
      },
      convertFileSrc: () => ''
    };
  });
}

(async () => {
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1
  });
  const page = await context.newPage();
  await fixture(page);
  await page.goto('http://127.0.0.1:1420');
  await page.locator('.workspace').waitFor();

  // 1. 文本工作区 & 智能对话
  await page.locator('.nav-item').filter({ hasText: '文本' }).click();
  await page.waitForTimeout(300);
  await page.getByRole('button', { name: '对话', exact: true }).click();
  await page.waitForTimeout(200);
  await page.getByRole('button', { name: /桌面端架构设计/ }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'chat.png') });
  console.log('✓ Captured chat.png');

  // 2. Agent 智能体与工具调用
  await page.getByRole('checkbox', { name: 'Agent', exact: true }).check();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'agent.png') });
  console.log('✓ Captured agent.png');

  // 3. 工具权限弹窗
  await page.getByRole('button', { name: '工具权限', exact: true }).click();
  await page.getByRole('dialog', { name: '工具权限' }).waitFor();
  await page.waitForTimeout(200);
  await page.screenshot({ path: path.join(outDir, 'permissions.png') });
  console.log('✓ Captured permissions.png');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);

  // 4. 图像生成工作台
  await page.locator('.nav-item').filter({ hasText: '图像' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'image-generation.png') });
  console.log('✓ Captured image-generation.png');

  // 5. 视频生成工作台
  await page.locator('.nav-item').filter({ hasText: '视频' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'video-generation.png') });
  console.log('✓ Captured video-generation.png');

  // 6. 音频生成工作台
  await page.locator('.nav-item').filter({ hasText: '音频' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'audio-generation.png') });
  console.log('✓ Captured audio-generation.png');

  // 7. 声音库管理中心
  await page.getByRole('button', { name: /^声音库 \(/ }).click();
  await page.getByRole('dialog', { name: '声音库' }).waitFor();
  await page.getByRole('button', { name: /新建声音/ }).click();
  await page.waitForTimeout(200);
  await page.screenshot({ path: path.join(outDir, 'voice-library.png') });
  console.log('✓ Captured voice-library.png');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);

  // 8. 自动化工作流管线
  await page.locator('.nav-item').filter({ hasText: '工作流' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'workflow.png') });
  console.log('✓ Captured workflow.png');

  // 9. 全局聚合检索
  await page.locator('.nav-item').filter({ hasText: '历史' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'history-search.png') });
  console.log('✓ Captured history-search.png');

  // 10. API 调试器
  await page.locator('.nav-item').filter({ hasText: '调试器' }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, 'api-debugger.png') });
  console.log('✓ Captured api-debugger.png');

  // 11. 模型管理中心
  await page.locator('.nav-item').filter({ hasText: '模型' }).click();
  await page.waitForTimeout(300);
  await page.locator('.catalog-list:visible li button').first().click();
  await page.waitForTimeout(200);
  await page.screenshot({ path: path.join(outDir, 'model-management.png') });
  console.log('✓ Captured model-management.png');

  // 12. 系统设置
  await page.locator('.nav-item').filter({ hasText: '设置' }).click();
  await page.waitForTimeout(300);
  await page.locator('.catalog-list:visible li button').first().click();
  await page.waitForTimeout(200);
  await page.screenshot({ path: path.join(outDir, 'settings.png') });
  console.log('✓ Captured settings.png');

  await browser.close();
  console.log('All 12 screenshots successfully updated to docs/screenshots!');
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
