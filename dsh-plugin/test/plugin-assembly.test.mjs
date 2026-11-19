/**
 * 装配冒烟测试（不依赖 LLM 与 daemon）：
 * 用假 ctx 调用插件 apply，验证在新版 DSH 运行时下：
 * - 7 个工具定义通过 dsh-tools 的 schema 校验（旧版曾因非法 schema 直接失败）
 * - agent/pre-step 与 session/event 监听器成功注册
 * - 自定义来源 kind 的消息可被 createUserMessage 构造
 */
import assert from 'node:assert/strict'
import { apply, Config } from '../lib/index.js'
import { createUserMessage } from '@deepseek-ai/dsh-llm'

const tools = []
const listeners = []
const fakeCtx = {
  tools: { register: (def) => { tools.push(def); return () => {} } },
  on: (event, handler, options) => { listeners.push({ event, handler, options }); return () => {} },
  get: () => undefined,
  logger: { warn: () => {}, info: () => {}, debug: () => {} },
}

const config = Config({})
apply(fakeCtx, config)

assert.equal(tools.length, 7, `应注册 7 个工具，实际 ${tools.length}`)
const names = tools.map((t) => t.name).sort()
assert.deepEqual(names, ['mr_add','mr_delete','mr_dream','mr_get','mr_list','mr_search','mr_stats'])
for (const t of tools) {
  assert.equal(t.output.schema.type, 'object', `${t.name} output schema 非法`)
  assert.equal(typeof t.execute, 'function')
}

const events = listeners.map((l) => l.event).sort()
assert.ok(events.includes('agent/pre-step'), `缺少 agent/pre-step，实际 ${events}`)
assert.ok(events.includes('session/event'), `缺少 session/event，实际 ${events}`)

// 验证注入消息构造（自定义 kind='memrec'）
const msg = createUserMessage({
  content: [{ type: 'text', text: 'x' }],
  source: { kind: 'memrec', form: 'snapshot', sections: [{ name: 'memrec-project-context', text: 'x' }] },
})
assert.equal(msg.role, 'user')
assert.equal(msg.source.kind, 'memrec')
assert.ok(typeof msg.id === 'string' && msg.id.length > 0)

console.log(JSON.stringify({ ok: true, tools: names, events }))
