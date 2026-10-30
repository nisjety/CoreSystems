class PCMWorkletProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const opts = (options && options.processorOptions) || {};
    this.targetSampleRate = Number(opts.targetSampleRate) || sampleRate;
  }

  downsampleBuffer(buffer, sourceRate, targetRate) {
    if (!buffer || buffer.length === 0) {
      return new Float32Array(0);
    }

    if (sourceRate === targetRate) {
      return buffer.slice();
    }

    const ratio = sourceRate / targetRate;
    const newLength = Math.max(1, Math.round(buffer.length / ratio));
    const result = new Float32Array(newLength);

    let offsetResult = 0;
    let offsetBuffer = 0;
    while (offsetResult < newLength) {
      const nextOffset = Math.round((offsetResult + 1) * ratio);
      let accum = 0;
      let count = 0;
      for (let i = offsetBuffer; i < nextOffset && i < buffer.length; i += 1) {
        accum += buffer[i];
        count += 1;
      }
      result[offsetResult] = count > 0 ? accum / count : 0;
      offsetResult += 1;
      offsetBuffer = nextOffset;
    }

    return result;
  }

  floatTo16BitPCM(buffer) {
    const output = new Int16Array(buffer.length);
    for (let i = 0; i < buffer.length; i += 1) {
      let sample = buffer[i];
      sample = Math.max(-1, Math.min(1, sample));
      output[i] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    }
    return output;
  }

  process(inputs, outputs) {
    const input = inputs && inputs[0];
    const output = outputs && outputs[0];

    if (Array.isArray(output) && output[0]) {
      output[0].fill(0);
    }

    if (!Array.isArray(input) || !input[0] || input[0].length === 0) {
      return true;
    }

    const channelData = input[0];
    let sumSquares = 0;
    for (let i = 0; i < channelData.length; i += 1) {
      const sample = channelData[i];
      sumSquares += sample * sample;
    }

    const rms = Math.sqrt(sumSquares / channelData.length);
    const targetRate = Math.min(this.targetSampleRate || sampleRate, sampleRate);
    const downsampled =
      targetRate === sampleRate
        ? channelData.slice()
        : this.downsampleBuffer(channelData, sampleRate, targetRate);

    if (downsampled.length === 0) {
      this.port.postMessage({ rms });
      return true;
    }

    const pcm = this.floatTo16BitPCM(downsampled);
    this.port.postMessage({ pcm: pcm.buffer, rms }, [pcm.buffer]);
    return true;
  }
}

registerProcessor('pcm-worklet-processor', PCMWorkletProcessor);





