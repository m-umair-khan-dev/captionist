# [OpenVINO EP] "Output names mismatch between OpenVINO and ONNX" on dynamically quantized (int8) NeMo FastConformer encoder

**Post to:** https://github.com/microsoft/onnxruntime/issues/new/choose (label: `ep:OpenVINO`)

---

### Describe the issue

Running an int8 NeMo Parakeet (FastConformer) encoder with the OpenVINO Execution Provider fails on the first inference:

```
terminate called after throwing an instance of 'Ort::Exception'
  what():  Non-zero status code returned while running OpenVINO-EP-subgraph_6 node.
  Name:'OpenVINOExecutionProvider_OpenVINO-EP-subgraph_6_5'
  Status Message: /onnxruntime/onnxruntime/core/providers/openvino/backend_utils.cc:101
  Ort::UnownedValue onnxruntime::openvino_ep::backend_utils::GetOutputTensor(Ort::KernelContext&, std::string,
  const onnxruntime::openvino_ep::SubGraphContext::string_index_map_t&, std::shared_ptr<ov::Node>)
  [OpenVINO-EP] Output names mismatch between OpenVINO and ONNX
```

The model was produced by ONNX Runtime dynamic quantization. It contains `DynamicQuantizeLinear` (160), `MatMulInteger` (154) and `ConvInteger` (56) nodes, and the graph is partitioned into several OpenVINO subgraphs. It runs correctly with the CPU Execution Provider.

What I observed:

| Case | Result |
|---|---|
| int8 encoder, OpenVINO EP `device_type=GPU` | Fails (error above) |
| int8 encoder, OpenVINO EP `device_type=CPU` | Fails (same error, same subgraph) |
| Same model exported in fp32, OpenVINO EP `device_type=GPU` | Works, correct output |
| int8 decoder/joiner of the same model (small LSTM/linear graphs) | Work |
| int8 encoder, CPU EP | Works |

So it depends on the quantized encoder graph, not on the device. Settings that made no difference:
- Session graph optimization level 0 (the default in my setup), 1 and 99
- `enable_qdq_optimizer=True`
- `disable_dynamic_shapes=True`
- `precision=FP32` and the default precision

Expected behaviour: either the subgraph runs, or the EP declines the unsupported nodes so they fall back to the CPU EP. It shouldn't throw at inference time. Because the exception escapes from `Run()` here, the host process aborts.

### To reproduce

Model: `encoder.int8.onnx` from sherpa-onnx's pre-converted NVIDIA Parakeet TDT 110M (CC-BY-4.0):
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8.tar.bz2
(file size 131,113,202 bytes, SHA-256 `0f35509ddeb9b39002fb077d979a9fe74f06eb0bc4dd5c34f512f82e5111d657`)

Inputs: `audio_signal` float32 `[batch, 80, frames]` (80-dim log-mel features) and `length` int64 `[batch]`. Outputs: `outputs`, `encoded_lengths`.

I hit it through sherpa-onnx's C API with provider `openvino`. The minimal Python script below should show the same thing. Note: I haven't run this exact script myself, because my system Python (3.14) has no `onnxruntime-openvino` wheel.

```python
import numpy as np
import onnxruntime as ort  # pip install onnxruntime-openvino==1.24.1

sess = ort.InferenceSession(
    "encoder.int8.onnx",
    providers=[("OpenVINOExecutionProvider", {"device_type": "CPU"})],  # GPU fails the same way
)
x = np.random.randn(1, 80, 400).astype(np.float32)  # ~4 s of features
n = np.array([400], dtype=np.int64)
print([o.shape for o in sess.run(None, {"audio_signal": x, "length": n})])
```

### Urgency

Not blocking: it works around by using the fp32 export, or by staying on the CPU EP. But it stops dynamically quantized speech models (a common way to ship them) from using OpenVINO at all. That includes Intel GPUs and NPUs, where users would benefit most.

### Platform

Linux

### OS Version

Ubuntu 26.10 (kernel 7.3.0), x86_64

### ONNX Runtime Installation

Released package: the native libraries from the `onnxruntime-openvino` 1.24.1 wheel (manylinux_2_28, cp312), loaded by sherpa-onnx 2.0.1 (C++ API), which I built against the ONNX Runtime 1.24.1 headers.

### ONNX Runtime Version or Commit ID

1.24.1

### ONNX Runtime API

C++ (sherpa-onnx uses the ONNX Runtime C++ API; the error is an `Ort::Exception`); Python repro above

### Architecture

X64

### Execution Provider

OpenVINO

### Execution Provider Library Version

OpenVINO 2025.4.1 (bundled in the wheel). GPU: Intel Graphics (Alder Lake-N, Intel N150, PCI 8086:46d4), compute runtime `intel-opencl-icd` 26.27.39122.14.
