<script lang="ts">
    import { setContext } from 'svelte'
    import Router, { push, type RouteDetail } from 'svelte-spa-router'
    import { wrap } from 'svelte-spa-router/wrap'
    import { get } from 'svelte/store'
    import { reloadServerInfo, serverInfo } from 'gateway/lib/store'
    import ThemeSwitcher from 'common/ThemeSwitcher.svelte'
    import DelayedSpinner from 'common/DelayedSpinner.svelte'
    import AuthBar from 'common/AuthBar.svelte'
    import Brand from 'common/Brand.svelte'
    import Loadable from 'common/Loadable.svelte'
    import { api, type AuthStateResponseInternal } from './lib/api'
    import { Button } from '@sveltestrap/sveltestrap'
    import Fa from 'svelte-fa'
    import { faArrowRight, faCog } from '@fortawesome/free-solid-svg-icons'
    import { hasAdminAccess } from 'admin/lib/store'

    interface Props {
        routePrefix?: string
        hideAdminButton?: boolean
        brandHref?: string
        profileHref?: string
        embedded?: boolean
    }

    let {
        routePrefix = '',
        hideAdminButton = false,
        brandHref,
        profileHref,
        embedded = false,
    }: Props = $props()

    function resolvedBrandHref (): string {
        return brandHref ?? `/@warpgate#${routePrefix || '/gateway'}`
    }

    function resolvedProfileHref (): string {
        return profileHref ?? `/@warpgate#${routePrefix || '/gateway'}/profile`
    }

    setContext('warpgate.profileHref', resolvedProfileHref)
    setContext('warpgate.gatewayRoutePrefix', () => routePrefix)
    setContext('warpgate.gatewayEmbedded', () => embedded)

    function prefixedRoute (path: string): string {
        return `${routePrefix}${path}`
    }

    let redirecting = $state(false)
    let serverInfoPromise = reloadServerInfo()
    let webAuthRequests: AuthStateResponseInternal[] = $state([])
    let doNotShowAuthRequests = $state(false)

    async function init () {
        await serverInfoPromise
    }

    function onPageResume () {
        redirecting = false
    }

    async function reloadWebAuthRequests () {
        webAuthRequests = await api.getWebAuthRequests()
    }

    async function requireLogin (detail: RouteDetail) {
        await serverInfoPromise
        if (!get(serverInfo)?.username) {
            const locationWithPrefix = routePrefix && !detail.location.startsWith(routePrefix)
                ? `${routePrefix}${detail.location}`
                : detail.location
            let url = location.pathname + '#' + locationWithPrefix
            if (detail.querystring) {
                url += '?' + detail.querystring
            }
            push(prefixedRoute('/login?next=' + encodeURIComponent(url)))
            return false
        }
        return true
    }

    const routes = {
        '/': wrap({
            asyncComponent: () => import('./TargetList.svelte') as any,
            props: {
                'on:navigation': () => redirecting = true,
            },
            conditions: [requireLogin],
        }),
        '/profile': wrap({
            asyncComponent: () => import('./Profile.svelte') as any,
            conditions: [requireLogin],
        }),
        '/profile/api-tokens': wrap({
            asyncComponent: () => import('./ProfileApiTokens.svelte') as any,
            conditions: [requireLogin],
        }),
        '/profile/credentials': wrap({
            asyncComponent: () => import('./ProfileCredentials.svelte') as any,
            conditions: [requireLogin],
        }),
        '/ticket-requests': wrap({
            asyncComponent: () => import('./TicketRequests.svelte') as any,
            conditions: [requireLogin],
        }),
        '/login': wrap({
            asyncComponent: () => import('./Login.svelte') as any,
        }),
        '/login/:stateId': wrap({
            asyncComponent: () => import('./OutOfBandAuth.svelte') as any,
            conditions: [requireLogin],
            userData: {
                doNotShowAuthRequests: true,
            },
        }),
    }

    const initPromise = init()
    let socket: WebSocket | null = null

    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-unused-expressions
        $serverInfo?.username // trigger effect on username change
        try {
            socket?.close()
        } catch {
            // ignore
        }
        socket = null
        if ($serverInfo?.username) {
            socket = new WebSocket(`wss://${location.host}/@warpgate/api/auth/web-auth-requests/stream`)
            socket.addEventListener('message', () => {
                reloadWebAuthRequests()
            })
            reloadWebAuthRequests()
        }
    })
</script>

<svelte:window on:pageshow={onPageResume}/>

<div class="gateway-app" class:standalone={!embedded} class:embedded>
    <Loadable promise={initPromise}>
        {#if redirecting}
            <DelayedSpinner />
        {:else}
            {#if !embedded}
            <div class="gateway-header">
                <a class="logo" href={resolvedBrandHref()}>
                    <Brand />
                </a>

                <div class="ms-auto d-flex align-items-center">
                    {#if !hideAdminButton && $hasAdminAccess}
                    <a href="/@warpgate" class="btn btn-warning btn-sm d-flex align-items-center gap-1 me-2">
                        <Fa icon={faCog} class="mx-1" />
                        <span class="me-1">Admin</span>
                    </a>
                    {/if}

                    <AuthBar />
                </div>
            </div>
            {/if}

            {#if !doNotShowAuthRequests}
            {#each webAuthRequests as authRequest (authRequest.id)}
                <Button
                    color="success"
                    class="mb-4 d-flex align-items-center w-100 text-start"
                    on:click={() => {
                        push(prefixedRoute('/login/' + authRequest.id))
                    }}
                >
                    <div>
                        <strong class="d-block">
                            {authRequest.protocol} authentication request
                        </strong>
                        {#if authRequest.address}
                            <small>
                                From {authRequest.address}
                            </small>
                        {/if}
                    </div>
                    <Fa class="ms-auto" icon={faArrowRight} />
                </Button>
            {/each}
            {/if}

            <main>
                <Router {routes} on:routeLoaded={e => {
                    doNotShowAuthRequests = !!(e.detail.userData as any)?.['doNotShowAuthRequests']
                }} prefix={routePrefix} />
            </main>

            {#if !embedded}
            <footer>
                {#if $serverInfo?.version}
                <span class="ms-3 me-auto">
                    {$serverInfo.version}
                </span>
                {:else}
                <div class="me-auto"></div>
                {/if}
                <ThemeSwitcher />
            </footer>
            {/if}
        {/if}
    </Loadable>
</div>

<style lang="scss">
    .gateway-app.standalone {
        width: min(720px, calc(100vw - 2rem));
        max-width: 100%;
        margin-left: auto;
        margin-right: auto;
    }

    .gateway-app.embedded {
        width: 100%;
    }

    .gateway-header {
        display: flex;
        align-items: center;
        min-height: 52px;
        margin: 0 0 1rem;
        border-bottom: 1px solid rgba(var(--bs-body-color-rgb), .2);
    }

    .logo {
        display: flex;
        align-items: center;
        flex: 0 0 auto;
        padding-right: 1rem;
    }
</style>
