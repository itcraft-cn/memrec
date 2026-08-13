/**
 * # daemon 客户端与工具集成测试
 *
 * 直连本机真实 memrecd（需已运行）验证：
 * - JSON-RPC 往返（ping / stats / add / search / delete）
 * - 工具定义与执行（含 working_dir 项目隔离）
 *
 * 运行：`node --test`（lib 需先构建）
 */
import { test } from 'node:test'
import assert from 'node:assert/strict'
import os from 'node:os'
import path from 'node:path'

import { MemrecDaemon } from '../lib/daemon.js'
import { registerMemrecTools, envFromExec } from '../lib/tools.js'
import { renderContextText } from '../lib/context.js'
import { parseExtraction } from '../lib/lifecycle.js'

const daemon = new MemrecDaemon(path.join(os.homedir(), '.memrec', 'memrecd.sock'))
const WORKING_DIR = '/disk2/helly_data/code/rust/memrec'

test('daemon ping 返回版本', async () => {
  const version = await daemon.ping()
  assert.ok(version !== null, 'memrecd 应可连接（请先启动 memrecd）')
})

test('daemon stats 返回统计', async () => {
  const result = await daemon.call('stats')
  assert.ok(typeof result === 'object' && result !== null)
  const stats = result
  assert.ok(
    'total_memories' in stats || 'active_memories' in stats,
    `stats 应含计数：${JSON.stringify(stats).slice(0, 200)}`,
  )
})

test('add + search + delete 往返（含 working_dir 项目隔离）', async () => {
  const marker = `dsh-plugin-test-${Date.now()}`
  const addResult = await daemon.call('add', {
    content: `DSH 插件集成测试记忆：${marker}`,
    memory_type: 'knowledge',
    tags: ['dsh-plugin-test'],
    working_dir: WORKING_DIR,
  })
  const memory = addResult?.memory
  assert.ok(memory?.id, `add 应返回 memory.id：${JSON.stringify(addResult)}`)
  const id = memory.id

  try {
    const search = await daemon.call('search_memory', {
      query: marker,
      top_k: 5,
      min_score: 0.3,
      project_only: true,
      working_dir: WORKING_DIR,
    })
    const hits = search?.results ?? []
    assert.ok(
      hits.some((h) => h.memory_id === id),
      `search 应命中刚添加的记忆：${JSON.stringify(hits)}`,
    )
  } finally {
    await daemon.call('delete', { id })
  }
})

test('envFromExec 从 agent session cwd 派生 working_dir', () => {
  const exec = { agent: { session: { header: { cwd: '/tmp/project' } } } }
  assert.equal(envFromExec(exec).workingDir, '/tmp/project')
})

test('envFromExec 无 agent 时回退 process.cwd()', () => {
  const exec = {}
  assert.equal(envFromExec(exec).workingDir, process.cwd())
})

test('工具注册：7 个 mr_* 工具定义完整', () => {
  const registered = []
  registerMemrecTools({ tools: { register: (d) => registered.push(d) } }, daemon)
  const names = registered.map((d) => d.name).sort()
  assert.deepEqual(names, [
    'mr_add',
    'mr_delete',
    'mr_dream',
    'mr_get',
    'mr_list',
    'mr_search',
    'mr_stats',
  ])
  for (const tool of registered) {
    assert.ok(tool.description.length > 10, `${tool.name} 应有描述`)
    assert.ok(tool.parameters, `${tool.name} 应有参数 schema`)
    // defineTool 编译产物：output schema 为合法 JSON Schema（object）
    assert.equal(tool.output.schema.type, 'object')
    assert.equal(typeof tool.execute, 'function')
  }
})

test('工具定义可通过 dsh-tools schema 校验（assertSupportedJsonSchema）', async () => {
  const { assertSupportedJsonSchema } = await import('@deepseek-ai/dsh-tools')
  const registered = []
  registerMemrecTools({ tools: { register: (d) => registered.push(d) } }, daemon)
  for (const tool of registered) {
    // 模拟 registry 对 output schema 的校验
    assert.doesNotThrow(() => assertSupportedJsonSchema(tool.output.schema))
    // 参数属性 type 均为受支持类型
    for (const prop of Object.values(tool.parameters.properties ?? {})) {
      assert.doesNotThrow(() => assertSupportedJsonSchema(prop), `${tool.name}.${prop?.description ?? ''}`)
    }
  }
})

test('mr_stats 工具执行返回统计', async () => {
  const registered = []
  registerMemrecTools({ tools: { register: (d) => registered.push(d) } }, daemon)
  const stats = registered.find((d) => d.name === 'mr_stats')
  const result = await stats.execute({}, {})
  assert.ok(typeof result === 'object' && result !== null)
})

test('mr_search 工具执行携带 working_dir 并返回结果', async () => {
  const registered = []
  registerMemrecTools({ tools: { register: (d) => registered.push(d) } }, daemon)
  const search = registered.find((d) => d.name === 'mr_search')
  const exec = { agent: { session: { header: { cwd: WORKING_DIR } } } }
  const result = await search.execute({ query: 'memrec', top_k: 3, project_only: true }, exec)
  assert.ok(result.error === undefined, `不应报错：${JSON.stringify(result)}`)
  assert.ok(Array.isArray(result.results), `应返回 results 数组`)
})

test('mr_add 工具空 content 报错', async () => {
  const registered = []
  registerMemrecTools({ tools: { register: (d) => registered.push(d) } }, daemon)
  const add = registered.find((d) => d.name === 'mr_add')
  const result = await add.execute({ content: '  ', memory_type: 'knowledge' }, {})
  assert.ok(typeof result.error === 'string' && result.error.includes('content'), '应返回错误')
})

test('renderContextText 渲染检索结果', () => {
  const text = renderContextText({
    results: [
      {
        score: 0.87,
        memory_type: 'decision',
        content_preview: '选择JWT认证方案',
        tags: ['auth', 'critical'],
      },
    ],
  })
  assert.ok(text.includes('MemRec 项目记忆'))
  assert.ok(text.includes('[0.87]'))
  assert.ok(text.includes('#auth'))
  assert.ok(text.includes('选择JWT认证方案'))
})

test('renderContextText 空结果返回空串', () => {
  assert.equal(renderContextText({ results: [] }), '')
  assert.equal(renderContextText({}), '')
})

test('parseExtraction 容错解析 LLM 输出', () => {
  assert.deepEqual(parseExtraction('[{"memory_type":"decision","content":"选A方案","tags":["a"]}]'), [
    { memory_type: 'decision', content: '选A方案', tags: ['a'] },
  ])
  assert.equal(parseExtraction('```json\n[]\n```').length, 0)
  assert.equal(parseExtraction('抱歉，我无法……').length, 0)
  // 无 content 字段的条目被过滤；空字符串 content 保留（写入侧再跳过）
  const mixed = parseExtraction('[{"memory_type":"decision"},{"content":""}]')
  assert.equal(mixed.length, 1)
  assert.equal(mixed[0].content, '')
})
