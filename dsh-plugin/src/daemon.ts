/**
 * # memrecd Unix Socket JSON-RPC 客户端
 *
 * 与 memrec 守护进程（memrecd）通信的轻量客户端。协议与 Rust 侧
 * `mr-protocol` 保持一致：
 *
 * - 传输：Unix Socket，路径默认 `~/.memrec/memrecd.sock`；
 * - 协议：JSON-RPC 2.0，一次请求一个连接；
 * - 流程：连接 → 写入请求 → `end()`（半关闭写端）→ 读取响应至 EOF → 解析；
 * - 方法名与 `mr_protocol::RequestAction` 的 snake_case 序列化一致，
 *   参数对象内嵌 `type` 内部标签（`#[serde(tag = "type")]`）。
 *
 * 连接失败（socket 不存在、daemon 未运行）抛出 [`MemrecDaemonError`]，
 * 由调用方决定降级策略，绝不导致 DSH 会话中断。
 */
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'

/** memrecd JSON-RPC 请求体。 */
export interface JsonRpcRequest {
  jsonrpc: '2.0'
  method: string
  params?: Record<string, unknown>
  id: number
}

/** memrecd JSON-RPC 响应体。 */
export interface JsonRpcResponse {
  jsonrpc: string
  result?: unknown
  error?: { code: number; message: string; data?: unknown }
  id: number
}

/** 与 memrecd 通信失败。message 面向用户可读。 */
export class MemrecDaemonError extends Error {
  readonly code: number

  constructor(message: string, code = -32603) {
    super(message)
    this.name = 'MemrecDaemonError'
    this.code = code
  }
}

/** 解析 `~` 前缀的用户路径。 */
function expandHome(p: string): string {
  if (p === '~') return os.homedir()
  if (p.startsWith('~/') || p.startsWith('~\\')) {
    return path.join(os.homedir(), p.slice(2))
  }
  return p
}

/** memrecd 客户端：一次请求一个 Unix Socket 连接。 */
export class MemrecDaemon {
  private nextId = 1

  /**
   * @param socketPath - socket 路径，支持 `~` 前缀展开
   */
  constructor(socketPath: string) {
    this.socketPath = expandHome(socketPath)
  }

  readonly socketPath: string

  /**
   * 默认 socket 路径：`~/.memrec/memrecd.sock`。
   */
  static defaultSocketPath(): string {
    return path.join(os.homedir(), '.memrec', 'memrecd.sock')
  }

  /**
   * 发送一个 JSON-RPC 请求并等待完整响应。
   *
   * 请求参数会注入 `type` 内部标签（与 Rust 端 `RequestParams` 的
   * `#[serde(tag = "type")]` 对应）。`params` 传 `undefined` 表示无参方法
   * （如 `stats`），不附加 `type`。
   *
   * @param method - 方法名（`add` / `search_memory` / `stats` 等）
   * @param params - 方法参数，自动注入 `type`
   * @returns 响应 `result` 字段
   * @throws {@link MemrecDaemonError} 连接失败或服务端返回 error
   */
  call(method: string, params?: Record<string, unknown>): Promise<unknown> {
    const id = this.nextId++
    const body: JsonRpcRequest = { jsonrpc: '2.0', method, id }
    if (params !== undefined) {
      body.params = { type: method, ...params }
    }

    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath)
      const chunks: Buffer[] = []
      let settled = false

      const fail = (err: Error) => {
        if (settled) return
        settled = true
        socket.destroy()
        reject(err)
      }

      socket.setTimeout(15_000)
      socket.on('timeout', () => {
        fail(new MemrecDaemonError(`memrecd 响应超时（${this.socketPath}）`))
      })

      socket.on('error', (err: NodeJS.ErrnoException) => {
        if (err.code === 'ENOENT' || err.code === 'ECONNREFUSED') {
          fail(
            new MemrecDaemonError(
              `memrecd 守护进程不可达：${this.socketPath}。请先启动 memrecd（memrecctl.sh start）`,
              -32099,
            ),
          )
        } else {
          fail(new MemrecDaemonError(`memrecd 连接失败：${err.message}`))
        }
      })

      socket.on('connect', () => {
        socket.write(JSON.stringify(body))
        // 半关闭写端，服务端读到 EOF 后返回响应（与 Rust 客户端 shutdown 语义一致）
        socket.end()
      })

      socket.on('data', (chunk: Buffer) => {
        chunks.push(chunk)
        // 响应上限 1MB，与 Rust 侧 MAX_BUFFER_SIZE 一致
        const total = chunks.reduce((n, c) => n + c.length, 0)
        if (total > 1024 * 1024) {
          fail(new MemrecDaemonError('memrecd 响应超过 1MB 上限'))
        }
      })

      socket.on('end', () => {
        if (settled) return
        settled = true
        const raw = Buffer.concat(chunks).toString('utf8')
        let response: JsonRpcResponse
        try {
          response = JSON.parse(raw) as JsonRpcResponse
        } catch (err) {
          reject(
            new MemrecDaemonError(
              `memrecd 响应解析失败：${err instanceof Error ? err.message : String(err)}`,
            ),
          )
          return
        }
        if (response.error !== undefined) {
          reject(
            new MemrecDaemonError(
              `memrecd 错误 ${response.error.code}: ${response.error.message}`,
              response.error.code,
            ),
          )
          return
        }
        resolve(response.result)
      })
    })
  }

  /**
   * 探测 daemon 可用性：发起 `get_version` 请求。
   * @returns 成功返回版本字符串，失败返回 null（不抛异常）
   */
  async ping(): Promise<string | null> {
    try {
      const result = (await this.call('get_version')) as { version?: string } | undefined
      return result?.version ?? 'ok'
    } catch {
      return null
    }
  }
}
