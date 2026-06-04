<script lang="ts">
    import Fa from 'svelte-fa'
    import { onDestroy, onMount } from 'svelte'
    import { Terminal } from '@xterm/xterm'
    import { SerializeAddon } from '@xterm/addon-serialize'
    import { faPlay, faPause, faExpand } from '@fortawesome/free-solid-svg-icons'
    import { Spinner } from '@sveltestrap/sveltestrap'
    import formatDuration from 'format-duration'
    import type { Recording } from 'admin/lib/api'

    export let recording: Recording

    const DEFAULT_FONT_SIZE = 20
    const MIN_FONT_SIZE = 1
    const MAX_FONT_SIZE = 48
    const FONT_SIZE_STEP = 0.25
    const FIT_TOLERANCE = 0.5

    let url: string
    let viewportElement: HTMLDivElement
    let containerElement: HTMLDivElement
    let rootElement: HTMLDivElement
    let timestamp = 0
    let seekInputValue = 0
    let duration = 0
    let events: (DataEvent | SizeEvent | SnapshotEvent)[] = []
    let playing = false
    let loading = true
    let resizeObserver: ResizeObserver | undefined
    let sessionIsLive: boolean | null = null
    let socket: WebSocket | null = null
    let isStreaming = false
    let isFullscreen = false
    let ptyMode = false
    let fitFrame: number | undefined
    let fitRunId = 0
    let lastViewportWidth = 0
    let lastViewportHeight = 0
    let windowedFontSize = DEFAULT_FONT_SIZE
    let metricsCanvas: HTMLCanvasElement | undefined
    let destroyed = false

    $: isStreaming = timestamp === duration && playing

    const COLOR_NAMES = [
        'black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
        'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite',
    ]

    const theme: Record<string, string> = {
        foreground: '#ffcb83',
        background: '#262626',
        cursor: '#fc531d',
    }
    const colors = [
        '#000000',
        '#c13900',
        '#a4a900',
        '#caaf00',
        '#bd6d00',
        '#fc5e00',
        '#f79500',
        '#ffc88a',
        '#6a4f2a',
        '#ff8c68',
        '#f6ff40',
        '#ffe36e',
        '#ffbe55',
        '#fc874f',
        '#c69752',
        '#fafaff',
    ]
    for (let i = 0; i < COLOR_NAMES.length; i++) {
        theme[COLOR_NAMES[i]!] = colors[i]!
    }

    interface AsciiCastHeader {
        time: number
        version: number
        width: number
        height: number
    }
    // eslint-disable-next-line @typescript-eslint/no-type-alias
    type AsciiCastData = [number, 'o', string]
    type AsciiCastItem = AsciiCastData | AsciiCastHeader

    function isAsciiCastHeader(data: AsciiCastItem): data is AsciiCastHeader {
        return 'version' in data
    }

    function isAsciiCastData(data: AsciiCastItem): data is AsciiCastData {
        if (data instanceof Array) {
            return data[1] === 'o' || data[1] === 'e'
        } else {
            return false
        }
    }

    interface SizeEvent { time: number, cols: number, rows: number }
    interface DataEvent { time: number, data: string }
    interface SnapshotEvent { time: number, snapshot: string }

    const term = new Terminal({
        fontFamily: 'monospace-fallback, monospace',
        fontSize: DEFAULT_FONT_SIZE,
        lineHeight: 1,
    })
    const serializeAddon = new SerializeAddon()

    onDestroy(() => {
        socket?.close()
        term.dispose()
        if (fitFrame !== undefined) {
            cancelAnimationFrame(fitFrame)
        }
    })

    onMount(async () => {
        if (recording.kind !== 'Terminal') {
            throw new Error('Invalid recording type')
        }

        url = `/@warpgate/admin/api/recordings/${recording.id}/cast`

        term.loadAddon(serializeAddon)
        term.open(containerElement)

        term.options.theme = theme
        term.options.scrollback = 100

        resizeObserver = new ResizeObserver(entries => {
            const entry = entries[0]
            if (!entry) {
                return
            }

            const width = Math.round(entry.contentRect.width * 100) / 100
            const height = Math.round(entry.contentRect.height * 100) / 100
            if (width === lastViewportWidth && height === lastViewportHeight) {
                return
            }

            lastViewportWidth = width
            lastViewportHeight = height
            scheduleFit()
        })
        resizeObserver.observe(viewportElement)
        document.addEventListener('fullscreenchange', handleFullscreenChange)

        const data = await fetch(url).then(r => r.text())
        for (const line of data.split('\n')) {
            addData(JSON.parse(line))
        }

        await primeTerminalSize(duration)
        await seek(duration)

        socket = new WebSocket(`wss://${location.host}/@warpgate/admin/api/recordings/${recording.id}/stream`)
        socket.addEventListener('message', function (event) {
            let message = JSON.parse(event.data)
            if ('data' in message) {
                let item: AsciiCastItem = message.data
                addData(item)
            } if ('start' in message) {
                sessionIsLive = message.live
                if (!sessionIsLive) {
                    seek(0)
                } else {
                    playing = true
                }
            } if ('end' in message) {
                sessionIsLive = false
            } else {
                console.log('Message from server ', message)
            }
        })
        socket.addEventListener('close', () => console.info('Live stream closed'))

        cancelScheduledFit()
        await fitSize()
        loading = false
    })

    async function writeToTerminal(data: string) {
        if (!ptyMode) {
            data = data.replace(/\n/g, '\r\n')
        }
        await new Promise<void>(r => term.write(data, r))
    }

    function addData(data: AsciiCastItem) {
        if (isAsciiCastHeader(data)) {
            if (data.width) {
                ptyMode = true
            }
            events.push({
                time: data.time,
                cols: data.width,
                rows: data.height,
            })
            if (isStreaming) {
                resize(data.width, data.height)
                timestamp = data.time
            }
            duration = Math.max(duration, data.time)
        }
        if (isAsciiCastData(data)) {
            let dataEvent = {
                time: data[0],
                data: data[2],
            }
            events.push(dataEvent)
            if (isStreaming) {
                writeToTerminal(dataEvent.data)
                timestamp = dataEvent.time
            }
            duration = Math.max(duration, dataEvent.time)
        }
    }

    function scheduleFit() {
        fitRunId += 1
        const scheduledFitRunId = fitRunId
        if (fitFrame !== undefined) {
            cancelAnimationFrame(fitFrame)
        }
        fitFrame = requestAnimationFrame(() => {
            fitFrame = undefined
            void fitSize(scheduledFitRunId)
        })
    }

    function cancelScheduledFit() {
        fitRunId += 1
        if (fitFrame !== undefined) {
            cancelAnimationFrame(fitFrame)
            fitFrame = undefined
        }
    }

    function roundFontSize(value: number) {
        return Math.round(value / FONT_SIZE_STEP) * FONT_SIZE_STEP
    }

    function clampFontSize(value: number) {
        return Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, value))
    }

    function measureCharacterWidth(fontSize: number) {
        metricsCanvas ??= document.createElement('canvas')
        const context = metricsCanvas.getContext('2d')
        if (!context) {
            return fontSize
        }

        context.font = `${fontSize}px ${term.options.fontFamily ?? 'monospace'}`
        return context.measureText('mmmmmmmmmm').width / 10
    }

    function getTerminalSizeAtTime(time: number) {
        let size = { cols: term.cols, rows: term.rows }
        for (const event of events) {
            if (event.time > time) {
                break
            }
            if ('cols' in event) {
                size = { cols: event.cols, rows: event.rows }
            }
        }
        return size
    }

    function estimateFontSize(cols: number, rows: number) {
        if (!viewportElement || !cols || !rows) {
            return null
        }

        const viewportRect = viewportElement.getBoundingClientRect()
        const screenElement = containerElement?.querySelector('.xterm-screen') as HTMLElement | null
        const screenRect = screenElement?.getBoundingClientRect()
        const availableWidth = screenRect?.width || viewportRect.width
        const availableHeight = screenRect?.height || viewportRect.height
        if (!availableWidth || !availableHeight) {
            return null
        }

        const measuredCharacterWidth = measureCharacterWidth(DEFAULT_FONT_SIZE)
        if (!measuredCharacterWidth) {
            return null
        }

        const lineHeight = Number(term.options.lineHeight ?? 1) || 1
        const widthFontSize = availableWidth * DEFAULT_FONT_SIZE / (measuredCharacterWidth * cols)
        const heightFontSize = availableHeight / (rows * lineHeight)
        return clampFontSize(roundFontSize(Math.min(widthFontSize, heightFontSize)))
    }

    async function primeTerminalSize(time: number) {
        cancelScheduledFit()
        await nextFrame()
        const size = getTerminalSizeAtTime(time)
        resize(size.cols, size.rows, false)

        const estimatedFontSize = estimateFontSize(size.cols, size.rows)
        if (estimatedFontSize === null) {
            return
        }

        term.options.fontSize = estimatedFontSize
        if (!isFullscreen) {
            windowedFontSize = estimatedFontSize
        }
        term.refresh(0, Math.max(term.rows - 1, 0))
        await nextFrame()
    }

    function getElementBottomRelativeTo(element: Element, containerRect: DOMRect) {
        return element.getBoundingClientRect().bottom - containerRect.top
    }

    function getElementRightRelativeTo(element: Element, containerRect: DOMRect) {
        return element.getBoundingClientRect().right - containerRect.left
    }

    function getMeasuredTerminalSize() {
        const xtermElement = containerElement?.querySelector('.xterm') as HTMLElement | null
        const screenElement = containerElement?.querySelector('.xterm-screen') as HTMLElement | null
        if (!xtermElement) {
            return null
        }

        const xtermRect = xtermElement.getBoundingClientRect()
        const screenRect = screenElement?.getBoundingClientRect()
        const rowContainer = screenElement?.querySelector('.xterm-rows') as HTMLElement | null
        const renderedRows = Array.from(screenElement?.querySelectorAll<HTMLElement>('.xterm-rows > div') ?? []).slice(0, term.rows)
        let contentWidth = Math.max(xtermRect.width, xtermElement.scrollWidth, screenRect?.width ?? 0, screenElement?.scrollWidth ?? 0)
        let contentHeight = screenRect?.height ?? xtermRect.height

        if (screenRect) {
            if (rowContainer) {
                contentWidth = Math.max(contentWidth, getElementRightRelativeTo(rowContainer, screenRect), rowContainer.scrollWidth)
            }
            for (const row of renderedRows) {
                contentWidth = Math.max(contentWidth, getElementRightRelativeTo(row, screenRect), row.scrollWidth)
                contentHeight = Math.max(contentHeight, getElementBottomRelativeTo(row, screenRect))
            }
        }

        return {
            width: contentWidth,
            height: Math.max(xtermRect.height, contentHeight),
        }
    }

    function nextFrame() {
        return new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
    }

    async function applyFontSize(fontSize: number, currentFitRunId: number) {
        if (destroyed || currentFitRunId !== fitRunId) {
            return false
        }

        const nextFontSize = roundFontSize(fontSize)
        const currentFontSize = Number(term.options.fontSize ?? DEFAULT_FONT_SIZE)
        if (Math.abs(currentFontSize - nextFontSize) < 0.01) {
            return true
        }

        term.options.fontSize = nextFontSize
        term.refresh(0, Math.max(term.rows - 1, 0))
        await nextFrame()
        return !destroyed && currentFitRunId === fitRunId
    }

    async function fitSize(currentFitRunId = fitRunId) {
        if (!viewportElement || !containerElement || !term.cols || !term.rows) {
            return
        }

        const viewportRect = viewportElement.getBoundingClientRect()
        const availableWidth = viewportRect.width
        const availableHeight = viewportRect.height
        if (!availableWidth || !availableHeight) {
            return
        }

        let renderedSize = getMeasuredTerminalSize()
        if (!renderedSize) {
            scheduleFit()
            return
        }

        const fitsViewport = (size: { width: number, height: number }) =>
            size.width <= availableWidth + FIT_TOLERANCE && size.height <= availableHeight + FIT_TOLERANCE

        const minimumFitStep = Math.round(MIN_FONT_SIZE / FONT_SIZE_STEP)
        let low = minimumFitStep
        let high = Math.round(MAX_FONT_SIZE / FONT_SIZE_STEP)
        let best = low

        while (low <= high) {
            const mid = Math.floor((low + high) / 2)
            const testFontSize = Math.max(MIN_FONT_SIZE, mid * FONT_SIZE_STEP)
            if (!await applyFontSize(testFontSize, currentFitRunId)) {
                return
            }

            renderedSize = getMeasuredTerminalSize()
            if (!renderedSize) {
                scheduleFit()
                return
            }

            if (fitsViewport(renderedSize)) {
                best = mid
                low = mid + 1
            } else {
                high = mid - 1
            }
        }

        const finalFontSize = Math.max(MIN_FONT_SIZE, best * FONT_SIZE_STEP)
        if (!await applyFontSize(finalFontSize, currentFitRunId)) {
            return
        }

        renderedSize = getMeasuredTerminalSize()
        while (renderedSize && !fitsViewport(renderedSize) && best > minimumFitStep) {
            best -= 1
            const decreasedFontSize = Math.max(MIN_FONT_SIZE, best * FONT_SIZE_STEP)
            if (!await applyFontSize(decreasedFontSize, currentFitRunId)) {
                return
            }
            renderedSize = getMeasuredTerminalSize()
        }

        if (!isFullscreen) {
            windowedFontSize = Math.max(MIN_FONT_SIZE, Number(term.options.fontSize ?? DEFAULT_FONT_SIZE))
        }
    }

    let seekPromise = Promise.resolve()

    async function seek(time: number) {
        seekPromise = seekPromise.then(() => _seekInternal(time))
        await seekPromise
    }

    async function _seekInternal(time: number) {
        let nearestSnapshot: SnapshotEvent | null = null

        for (const event of events) {
            if (event.time > time) {
                break
            }
            if ('snapshot' in event) {
                nearestSnapshot = event
            }
        }

        let index = nearestSnapshot ? events.indexOf(nearestSnapshot) : 0
        if (time >= timestamp) {
            const nextEventIndex = events.findIndex(e => e.time > timestamp)
            if (nextEventIndex === -1) {
                return
            }
            index = Math.max(index, nextEventIndex)
        }
        let lastSize = { cols: term.cols, rows: term.rows }

        for (let i = 0; i <= index; i++) {
            let event = events[i]!
            if ('cols' in event) {
                lastSize = { cols: event.cols, rows: event.rows }
            }
        }

        resize(lastSize.cols, lastSize.rows)

        let output = ''

        async function flush() {
            await writeToTerminal(output)
            output = ''
        }

        for (let i = index; i < events.length; i++) {
            let shouldSnapshot = false
            let event = events[i]!
            if (event.time > time) {
                break
            }
            if ('snapshot' in event) {
                output += '\x1bc' + event.snapshot
            }
            if ('cols' in event) {
                await flush()
                resize(event.cols, event.rows)
                shouldSnapshot = true
            }
            if ('data' in event) {
                output += event.data
            }

            shouldSnapshot ||= output.length > 1000

            if (shouldSnapshot) {
                await flush()
                events.splice(i + 1, 0, {
                    time: event.time,
                    snapshot: serializeAddon.serialize(),
                })
                i++
            }
        }

        await flush()

        timestamp = time
        seekInputValue = 100 * time / duration
    }

    function resize(cols: number, rows: number, schedule = true) {
        let resized = false
        if (cols && rows) {
            if (term.cols !== cols || term.rows !== rows) {
                term.resize(cols, rows)
                resized = true
            }
        }
        if (resized && schedule) {
            scheduleFit()
        }
    }

    onDestroy(() => {
        document.removeEventListener('fullscreenchange', handleFullscreenChange)
        resizeObserver?.disconnect()
    })

    onDestroy(() => destroyed = true)

    async function step() {
        if (destroyed) {
            return
        }
        if (playing) {
            await seek(Math.min(duration, timestamp + 0.1))
        }
        setTimeout(step, 100)
    }

    function togglePlaying() {
        playing = !playing
    }

    function keyPressHandler(event: KeyboardEvent) {
        if (event.key === ' ') {
            togglePlaying()
        }
    }

    step()

    function handleFullscreenChange() {
        isFullscreen = document.fullscreenElement === rootElement
        if (!isFullscreen) {
            term.options.fontSize = windowedFontSize
        }
        scheduleFit()
    }

    function toggleFullscreen() {
        if (document.fullscreenElement) {
            document.exitFullscreen()
        } else {
            rootElement.requestFullscreen()
        }
    }
</script>

<div
    class="root"
    class:fullscreen={isFullscreen}
    bind:this={rootElement}
    style="background: {theme.background}"
>
    {#if loading}
    <Spinner color="primary" />
    {/if}

    <div
        class="pause-overlay"
        class:invisible={loading || playing}
        on:click={togglePlaying}
        on:keypress={keyPressHandler}
        role="button"
        tabindex="0"
    >
        <Fa icon={faPlay} size="2x" fw />
    </div>

    <div
        class="viewport"
        class:invisible={loading}
        bind:this={viewportElement}
    >
        <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
        <div
            class="terminal-frame"
            on:click={togglePlaying}
            on:keypress={keyPressHandler}
            role="img"
            bind:this={containerElement}
        ></div>
    </div>

    <div class="toolbar" class:invisible={loading}>
        <button class="btn btn-link" on:click|stopPropagation={togglePlaying}>
            <Fa icon={playing ? faPause : faPlay} fw />
        </button>
        <pre
            class="timestamp"
        >{ formatDuration(timestamp * 1000, { leading: true }) }</pre>
        {#if sessionIsLive === true}
            <button
                class="btn live-btn"
                class:active={isStreaming}
                on:click|stopPropagation={() => seek(duration)}
            >LIVE</button>
        {/if}
        <input
            class="seek-input"
            type="range"
            min="0" max="100" step="0.001"
            style="background-size: {seekInputValue}% 100%;"
            bind:value={seekInputValue}
            on:click|stopPropagation
            on:input={() => seek(duration * seekInputValue / 100)} />
        <button class="btn btn-link" on:click|stopPropagation={toggleFullscreen}>
            <Fa icon={faExpand} fw />
        </button>
    </div>
</div>

<style lang="scss">
    @import "../../../node_modules/@xterm/xterm/css/xterm.css";

    .root {
        border-radius: 5px;
        overflow: hidden;
        position: relative;
        contain: content;
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
        min-width: 0;
        max-height: 100%;
        max-width: 100%;
        width: 100%;
    }

    .root.fullscreen {
        border-radius: 0;
    }

    .viewport {
        flex: 1 1 0;
        min-height: 0;
        max-height: 100%;
        overflow: hidden;
        padding: 0;
        position: relative;
        width: 100%;
        display: flex;
        flex-direction: column;
    }

    .terminal-frame {
        flex: 1;
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
        max-height: 100%;
        max-width: 100%;
        overflow: hidden;
    }

    .toolbar {
        align-items: center;
        display: flex;
        flex: none;
        min-width: 0;
        overflow: hidden;
    }

    :global(.xterm) {
        cursor: pointer !important;
        width: 100% !important;
        height: 100% !important;
        padding: 0;
        background: #262626;
    }

    :global(.xterm-screen) {
        width: 100% !important;
        height: 100% !important;
        max-width: 100%;
        max-height: 100%;
        overflow: hidden;
    }

    :global(.xterm-viewport) {
        width: 100% !important;
        height: 100% !important;
        max-width: 100%;
        max-height: 100%;
        background: #262626 !important;
        overflow-y: hidden !important;
    }

    :global(.xterm-scrollable-element) {
        width: 100% !important;
        height: 100% !important;
        max-width: 100%;
        max-height: 100%;
        overflow: hidden;
    }

    :global(.xterm .scrollbar) {
        display: none !important;
    }

    .invisible {
        visibility: hidden;
    }

    .btn {
        color: #eee;

        :global(svg) {
            transition: all .25s ease-out;
            &:hover {
                transform: scale(1.2);
            }
        }
    }

    :global(.spinner-border), .pause-overlay {
        position: absolute;
        left: 50%;
        top: 50%;
        margin: -12px 0 0 -12px;
        z-index: 1;
    }

    .pause-overlay {
        width: 24px;
        text-align: center;
        color: white;
    }

    .root.fullscreen .viewport {
        padding: 0;
    }

    input[type="range"] {
        appearance: none;
        -webkit-appearance: none;
        margin: 18px 10px 0;
        height: 2px;
        background: #ffffff99;
        border-radius: 5px;
        background: linear-gradient(#eee, #eee);
        background-repeat: no-repeat;
        cursor: pointer;

        &:hover::-webkit-slider-thumb {
            transform: scale(1.5);
        }
    }

    .seek-input {
        flex: 1 1 auto;
        min-width: 0;
        width: auto;
    }

    input[type="range"]::-webkit-slider-thumb {
        -webkit-appearance: none;
        height: 10px;
        width: 10px;
        border-radius: 50%;
        background: #eee;
        transition: all .25s ease-out;
    }

    input[type=range]::-webkit-slider-runnable-track {
        -webkit-appearance: none;
        box-shadow: none;
        border: none;
        background: transparent;
    }

    .timestamp {
        flex: none;
        overflow: visible;
        color: #eeeeee;
        margin: 0;
        font-size: 0.75rem;
        align-self: center;
    }

    .live-btn {
        font-size: 0.75rem;
        align-self: center;
        color: red;
        flex: none;

        &.active {
            background: red;
            color: white;
            padding: 0.1rem 0.25rem;
            margin: 0 0.5rem;
        }
    }
</style>
