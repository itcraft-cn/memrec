#!/usr/bin/env bash
#
# 检索质量门禁（retrieval gate）
#
# 用途：作为多步骤测试与部署流程中的强制质量关卡，每次改动后校验检索排序质量，
#       防止「向量归一化方向、中文分词、MMR 重排」等导致的相关性回归。
#
# 校验三部分：
#   1. 自检索（纯向量、关闭 MMR）：抽样记忆，以其正文片段为查询，要求能检索到自身。
#      这一项直接锁定向量通路「最相似者优先」的不变量，能捕获归一化方向反转。
#   2. 语义金标集（混合检索）：eval/retrieval_golden.jsonl 中的改写查询，要求
#      hit@k 与 MRR 达标，衡量真实语义召回与排序质量。
#   3. 负例（无关查询）：完全无关的查询不得因个别字词碰撞得到高置信结果
#      （top1 校准分必须低于默认阈值），锁定「单路 BM25 命中不再等于 0.5」；
#      且命中不得是空内容记忆（空串嵌入曾长期污染无关查询首位）。
#   4. top-k 前缀一致性：小 k 的结果必须是同查询大 k 结果的前缀，
#      锁定「候选池 over-fetch、融合后再截断」的不变量，防止 top1 随 k 漂移。
#
# 依赖：memrecd 正在运行；PATH 中存在 memrec 与 jq。
# 退出码：0=通过；非 0=任一项未达标（可用于 CI / 部署流程阻断）。
#
# 阈值可用环境变量覆盖（见下方默认值）。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MEMREC_BIN="${MEMREC_BIN:-memrec}"
GOLDEN="${REPO_ROOT}/eval/retrieval_golden.jsonl"

K="${RETRIEVAL_GATE_K:-5}"
SELF_SAMPLE="${RETRIEVAL_GATE_SELF_SAMPLE:-120}"

GOLDEN_MIN_HIT="${RETRIEVAL_GATE_MIN_HIT:-0.80}"
GOLDEN_MIN_MRR="${RETRIEVAL_GATE_MIN_MRR:-0.55}"
SELF_MIN_RECALL="${RETRIEVAL_GATE_MIN_SELF_RECALL:-0.97}"
NEG_MAX_TOP="${RETRIEVAL_GATE_NEG_MAX_TOP:-0.60}"

command -v "$MEMREC_BIN" >/dev/null || { echo "找不到 memrec 可执行文件: $MEMREC_BIN"; exit 2; }
command -v jq >/dev/null || { echo "缺少 jq"; exit 2; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# 数值比较：a >= b 返回 0。
ge() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a >= b) }'; }

fail=0

# ---------- 1. 自检索（纯向量、no-mmr） ----------
self_ds="$tmp/self.jsonl"
"$MEMREC_BIN" list -a -l 400 > "$tmp/list.json"
jq -r '.result.memories[]
       | select((.content | length) >= 40)
       | {id: .id, query: (.content[0:200]), relevant: [.id]}
       | @json' "$tmp/list.json" | shuf -n "$SELF_SAMPLE" > "$self_ds"

self_n="$(wc -l < "$self_ds" | tr -d ' ')"
echo "[retrieval-gate] 自检索：$self_n 条（纯向量 alpha=1.0 / no-mmr / k=$K）"
if [ "$self_n" -gt 0 ]; then
  self_report="$("$MEMREC_BIN" eval "$self_ds" --k "$K" --alpha 1.0 --no-mmr 2>/dev/null)"
  self_recall="$(jq -r '.mean_recall' <<<"$self_report")"
  echo "[retrieval-gate]   自检索 mean_recall@$K = $self_recall（阈值 >= $SELF_MIN_RECALL）"
  if ! ge "$self_recall" "$SELF_MIN_RECALL"; then
    echo "[retrieval-gate]   失败：自检索召回不足，疑似向量排序方向错误或索引缺失"
    fail=1
  fi
else
  echo "[retrieval-gate]   跳过：无足量记忆可抽样"
fi

# ---------- 2. 语义金标集（混合检索） ----------
echo "[retrieval-gate] 金标集：$GOLDEN（混合检索 alpha=0.5 / MMR，k=$K）"
golden_report="$("$MEMREC_BIN" eval "$GOLDEN" --k "$K" 2>/dev/null)"
hit="$(jq -r '.hit_rate' <<<"$golden_report")"
mrr="$(jq -r '.mrr' <<<"$golden_report")"
echo "[retrieval-gate]   hit@$K = $hit（阈值 >= $GOLDEN_MIN_HIT）"
echo "[retrieval-gate]   MRR   = $mrr（阈值 >= $GOLDEN_MIN_MRR）"
if ! ge "$hit" "$GOLDEN_MIN_HIT"; then
  echo "[retrieval-gate]   失败：hit@$K 未达标"
  fail=1
fi
if ! ge "$mrr" "$GOLDEN_MIN_MRR"; then
  echo "[retrieval-gate]   失败：MRR 未达标"
  fail=1
fi

# ---------- 3. 负例（无关查询不得给出高置信结果） ----------
# 以 min_score=0 观察原始排序，要求 top1 校准分严格低于默认阈值。
# 该用例锁定"单路词项碰撞不再等价于 0.5"的不变量。
neg_queries=(
  "如何给宠物仓鼠挑选跑轮"
  "如何制作抹茶提拉米苏甜点"
  "深海热泉喷口附近的化能合成生态系统有哪些物种"
  "红酒炖牛肉的传统做法"
  "如何给吉他更换琴弦并调音"
  "马拉松赛前的碳水储备策略"
  "中世纪城堡的护城河是如何供水的"
  "如何辨别天然蜂蜜与浓缩蜂蜜"
)
for q in "${neg_queries[@]}"; do
  payload="$("$MEMREC_BIN" search "$q" -a --min-score 0 -k 1 2>/dev/null || echo '{}')"
  top="$(jq -r '.result.results[0].score // 0' <<<"$payload" 2>/dev/null || echo 0)"
  preview="$(jq -r '.result.results[0].content_preview // ""' <<<"$payload" 2>/dev/null || echo "")"
  echo "[retrieval-gate] 负例：\"$q\" top1=$top（要求 < $NEG_MAX_TOP 且非空内容）"
  if ! awk -v a="$top" -v b="$NEG_MAX_TOP" 'BEGIN { exit !(a < b) }'; then
    echo "[retrieval-gate]   失败：无关查询 top1 达到或超过阈值"
    fail=1
  fi
  if [ -z "$preview" ]; then
    echo "[retrieval-gate]   失败：无关查询 top1 是空内容记忆（索引污染）"
    fail=1
  fi
done

# ---------- 4. top-k 前缀一致性 ----------
# 同一查询下，k=3 的结果集合必须被 k=10 的结果集合包含，且 top1 相同。
# 该不变量依赖"候选池与返回条数解耦"：若候选池仍等于 top_k，小 k 会看到
# 不同的融合归一化结果，top1 可能反而更差。
prefix_queries=(
  "memrec 检索阈值"
  "Dream 融合质量"
  "Spring Cloud Gateway 限流配置"
)
for q in "${prefix_queries[@]}"; do
  ids3="$("$MEMREC_BIN" search "$q" -a --min-score 0 -k 3 2>/dev/null | jq -r '.result.results[].memory_id')"
  ids10="$("$MEMREC_BIN" search "$q" -a --min-score 0 -k 10 2>/dev/null | jq -r '.result.results[].memory_id')"
  missing="$(comm -23 <(sort <<<"$ids3") <(sort <<<"$ids10"))"
  top3="$(head -n1 <<<"$ids3")"
  top10="$(head -n1 <<<"$ids10")"
  echo "[retrieval-gate] 前缀：\"$q\" k3⊆k10=$([ -z "$missing" ] && echo yes || echo no) top1一致=$([ "$top3" = "$top10" ] && echo yes || echo no)"
  if [ -n "$missing" ] || [ "$top3" != "$top10" ]; then
    echo "[retrieval-gate]   失败：top-k 结果不满足前缀一致性"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "[retrieval-gate] 未通过"
  exit 1
fi
echo "[retrieval-gate] 通过"
