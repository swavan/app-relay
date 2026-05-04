<script lang="ts">
  import type { ViewportSize } from "./services";
  import type { VideoStreamSession } from "./videoStreams";
  import { buildVideoRendererView } from "./videoRenderer";

  export let stream: VideoStreamSession | null = null;
  export let requestedViewport: ViewportSize | null = null;
  export let mediaStream: MediaStream | null = null;

  let videoEl: HTMLVideoElement | null = null;

  $: renderer = buildVideoRendererView(stream, requestedViewport);
  $: if (videoEl && mediaStream) {
    videoEl.srcObject = mediaStream;
  }
</script>

<section class={`video-renderer ${renderer.state}`} aria-label="Video stream renderer">
  <div class="viewport">
    <div class="viewport-topline">
      <span>{renderer.stateLabel}</span>
      <span>{renderer.pipelineLabel}</span>
    </div>

    <div class="viewport-center">
      {#if mediaStream}
        <video
          class="webrtc-video"
          autoplay
          muted
          playsinline
          bind:this={videoEl}
        >
          <track kind="captions" />
        </video>
      {:else if renderer.hasEncodedFrame}
        <strong>{renderer.windowTitle}</strong>
        <span>{renderer.lastFrameLabel}</span>
      {:else}
        <strong>{renderer.emptyHeading}</strong>
        <span>{renderer.emptyDetail}</span>
      {/if}
    </div>

    <div class="viewport-footer">
      <span>{renderer.targetResolutionLabel}</span>
      <span>{renderer.frameRateLabel}</span>
      <span>{renderer.bitrateLabel}</span>
    </div>
  </div>

  <div class="metadata" aria-label="Video stream metadata">
    <div class="metadata-header">
      <div>
        <span>Selected window</span>
        <strong>{renderer.windowTitle}</strong>
      </div>
      <span class="state-pill">{renderer.stateLabel}</span>
    </div>

    <div class="metadata-grid">
      <div>
        <span>Stream</span>
        <strong>{renderer.streamIdLabel}</strong>
      </div>
      <div>
        <span>Window</span>
        <strong>{renderer.selectedWindowLabel}</strong>
      </div>
      <div>
        <span>Requested</span>
        <strong>{renderer.requestedResolutionLabel}</strong>
      </div>
      <div>
        <span>Viewport</span>
        <strong>{renderer.viewportResolutionLabel}</strong>
      </div>
      <div>
        <span>Target</span>
        <strong>{renderer.targetResolutionLabel}</strong>
      </div>
      <div>
        <span>Adaptation</span>
        <strong>{renderer.adaptationLabel}</strong>
      </div>
      <div>
        <span>Encoding</span>
        <strong>{renderer.codecLabel}</strong>
      </div>
      <div>
        <span>Encoded frames</span>
        <strong>{renderer.framesLabel}</strong>
      </div>
      <div>
        <span>Capture</span>
        <strong>{renderer.captureRuntimeLabel}</strong>
      </div>
      <div>
        <span>Capture frames</span>
        <strong>{renderer.captureFramesLabel}</strong>
      </div>
      <div>
        <span>Last capture</span>
        <strong>{renderer.captureLastFrameLabel}</strong>
      </div>
      <div>
        <span>Capture note</span>
        <strong>{renderer.captureMessageLabel}</strong>
      </div>
      <div>
        <span>Keyframes</span>
        <strong>{renderer.keyframesLabel}</strong>
      </div>
      <div>
        <span>Bytes</span>
        <strong>{renderer.bytesLabel}</strong>
      </div>
      <div>
        <span>Latency</span>
        <strong>{renderer.latencyLabel}</strong>
      </div>
      <div>
        <span>Reconnects</span>
        <strong>{renderer.reconnectsLabel}</strong>
      </div>
    </div>

    <p>{renderer.healthLabel}</p>
  </div>
</section>

<style>
  .video-renderer {
    background: #172026;
    border: 1px solid #2f3d46;
    border-radius: 8px;
    color: #f8fafc;
    display: grid;
    gap: 14px;
    grid-template-columns: minmax(260px, 1.35fr) minmax(260px, 1fr);
    margin: 20px 0;
    padding: 14px;
  }

  .video-renderer.empty,
  .video-renderer.stopped {
    background: #20252b;
  }

  .video-renderer.failed {
    border-color: #b42318;
  }

  .viewport {
    aspect-ratio: 16 / 9;
    background:
      linear-gradient(90deg, rgba(255, 255, 255, 0.06) 1px, transparent 1px),
      linear-gradient(rgba(255, 255, 255, 0.06) 1px, transparent 1px),
      #0b1117;
    background-size: 40px 40px;
    border: 1px solid #334155;
    border-radius: 6px;
    box-sizing: border-box;
    display: grid;
    grid-template-rows: auto 1fr auto;
    min-height: 220px;
    overflow: hidden;
    padding: 12px;
  }

  .viewport-topline,
  .viewport-footer {
    color: #cbd5e1;
    display: flex;
    flex-wrap: wrap;
    font-size: 0.78rem;
    gap: 10px;
    justify-content: space-between;
  }

  .viewport-center {
    align-content: center;
    display: grid;
    gap: 8px;
    justify-items: center;
    min-width: 0;
    text-align: center;
  }

  .viewport-center strong {
    font-size: 1.35rem;
    max-width: 100%;
    overflow-wrap: anywhere;
  }

  .viewport-center span {
    color: #93c5fd;
    overflow-wrap: anywhere;
  }

  .webrtc-video {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .metadata {
    display: grid;
    gap: 12px;
    min-width: 0;
  }

  .metadata-header {
    align-items: start;
    display: flex;
    gap: 12px;
    justify-content: space-between;
  }

  .metadata-header div,
  .metadata-grid div {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .metadata span {
    color: #cbd5e1;
    font-size: 0.76rem;
  }

  .metadata strong {
    overflow-wrap: anywhere;
  }

  .state-pill {
    background: #0f766e;
    border-radius: 999px;
    color: #ffffff;
    flex: 0 0 auto;
    padding: 4px 8px;
  }

  .failed .state-pill {
    background: #b42318;
  }

  .stopped .state-pill,
  .empty .state-pill {
    background: #475569;
  }

  .metadata-grid {
    display: grid;
    gap: 10px;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .metadata p {
    color: #93c5fd;
    margin: 0;
    overflow-wrap: anywhere;
  }

  @media (max-width: 820px) {
    .video-renderer {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 520px) {
    .metadata-grid {
      grid-template-columns: 1fr;
    }

    .viewport {
      min-height: 190px;
    }
  }
</style>
