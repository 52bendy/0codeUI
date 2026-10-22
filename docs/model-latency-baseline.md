# 模型推理延迟 / 成本基线

> 对应需求：REQ-202610-1181
>
> 工具：`intimeui-model-bench`（直接调用 `ModelGateway`，包含并发控制、重试与单文件 HTML 校验）
>
> 方法：`docs/model-inference-deployment.md` §5

## 1. 本次测量（mock 链路基线）

本次在无 GPU 的 CI / 开发服务器上运行 mock provider，仅用于验证测量链路、指标口径与
报告格式，**不代表真实模型性能**。

| 项目 | 值 |
| --- | --- |
| 生成时间 | 2026-10-22T07:52:21Z |
| 提供者 | `mock` |
| 主模型 | `Qwen2.5-Coder-7B-Instruct-AWQ` |
| 请求数 / 并发 | 100 / 4 |
| 成功 / 失败 | 100 / 0 |
| 单文件 HTML 合法 | 100 |
| P50 / P95 / P99 | 6.483 / 6.842 / 7.041 ms |
| 平均 / 最小 / 最大 | 6.389 / 5.242 / 7.049 ms |
| 吞吐 | 593.678 req/s |
| 输入 / 输出 token | 17,100 / 65,500 |
| 估算成本 | $0.00（自托管价格系数为 0） |

原始数据：`docs/model-latency-baseline.json`。

> mock 延迟由 `MODEL_MOCK_LATENCY_MS`（默认 30ms）与调度开销构成；本次使用默认配置，
> 数值主要反映网关与测量工具开销，不能用于容量规划。

## 2. 真实模型测量模板（24GB GPU）

在目标机器按 `docs/model-inference-deployment.md` §3 启动 vLLM 后，执行：

```bash
MODEL_PROVIDER=openai_compatible \
MODEL_BASE_URL=http://127.0.0.1:8000/v1 \
MODEL_NAME=Qwen2.5-Coder-7B-Instruct-AWQ \
MODEL_MAX_CONCURRENCY=4 \
cargo run --release -p intimeui-server --bin intimeui-model-bench -- \
  --requests 100 --concurrency 4 \
  --output docs/model-latency-baseline.json \
  --markdown docs/model-latency-baseline.md
```

建议记录以下环境信息（每次发布必须固定）：

| 项目 | 实测值（待 24GB 机器回填） |
| --- | --- |
| GPU 型号 / 显存 | 例如 RTX 4090 24GB |
| NVIDIA Driver / CUDA | 例如 550.54 / 12.4 |
| vLLM 镜像 tag / digest | `vllm/vllm-openai:v0.8.5` / `<digest>` |
| 模型 revision | `<commit>` |
| vLLM 参数 | `--max-model-len 8192 --gpu-memory-utilization 0.90 --max-num-seqs 8` |
| 冷启动时间 | 从容器启动到 `/health` 通过 |
| 单并发 P50 / P95 | 待测 |
| 并发 4 P50 / P95 / P99 | 待测 |
| 吞吐 | 待测 |
| 输入 / 输出 token（100 请求） | 待测 |
| 估算成本 | 自托管为 0；外部 API 按价格系数计算 |

## 3. 判定标准

| 指标 | 目标 | 说明 |
| --- | --- | --- |
| 合法 HTML 率 | ≥ 99% | 由网关 `extract_and_validate` 判定 |
| 单并发 P95 | 记录实测值 | PRD §6.1 端到端目标 ≤1.5s，按实际模型能力评估 |
| 并发 4 错误率 | < 1% | 不含主动 429 / 503 排队拒绝 |
| 超时率 | < 1% | `MODEL_REQUEST_TIMEOUT_MS` 内完成 |
| 成本 | 可解释 | 自托管统计 token；外部 API 按输入 / 输出单价估算 |

## 4. 成本公式

```
cost_usd = input_tokens / 1000 * MODEL_PRICE_INPUT_PER_1K
         + output_tokens / 1000 * MODEL_PRICE_OUTPUT_PER_1K
```

自托管默认两项为 0，仅统计 token 量；若使用外部 DeepSeek / OpenAI 兼容 API，按供应商
价格填入环境变量，`/api/v1/ui/model/metrics` 会累计估算成本。

## 5. 结论与后续

- 网关链路、并发 / 排队、SSE 流式、指标与 HTML 校验已由自动化测试覆盖；
- mock 基线证明测量工具可重复运行，真实模型基线需在 24GB GPU 机器执行 §2 并回填；
- 每次升级模型、vLLM、驱动或修改 `max-model-len` / `max-num-seqs` 后应重跑并对比 P95 与错误率。
