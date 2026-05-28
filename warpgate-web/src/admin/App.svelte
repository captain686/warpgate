<script lang="ts">
    import { setContext } from 'svelte'
    import { get } from 'svelte/store'
    import { serverInfo, reloadServerInfo } from 'gateway/lib/store'

    import Router, { link, location as routeLocation, push, type RouteDetail, type WrappedComponent } from 'svelte-spa-router'
    import active from 'svelte-spa-router/active'
    import { wrap } from 'svelte-spa-router/wrap'
    import ThemeSwitcher from 'common/ThemeSwitcher.svelte'
    import AuthBar from 'common/AuthBar.svelte'
    import Brand from 'common/Brand.svelte'
    import Loadable from 'common/Loadable.svelte'

    setContext('warpgate.profileHref', '/@warpgate#/gateway/profile')

    async function init () {
        await reloadServerInfo()
    }

    const initPromise = init()
    let fullScreenRoute = $derived($routeLocation.startsWith('/gateway/web-ssh/'))

    async function requireLogin (detail: RouteDetail) {
        await initPromise
        if (!get(serverInfo)?.username) {
            let url = location.pathname + '#' + detail.location
            if (detail.querystring) {
                url += '?' + detail.querystring
            }
            push('/gateway/login?next=' + encodeURIComponent(url))
            return false
        }
        return true
    }

    const routes: Record<string, WrappedComponent> = {
        '/': wrap({
            asyncComponent: () => import('./Home.svelte') as any,
            conditions: [requireLogin],
        }),
        '/gateway/web-ssh/:sessionId': wrap({
            asyncComponent: () => import('../gateway/WebSsh.svelte') as any,
            conditions: [requireLogin],
        }),
        '/profile': wrap({
            asyncComponent: () => import('common/RouteRedirect.svelte') as any,
            props: {
                to: '/gateway/profile',
            },
        }),
        '/sessions/:id': wrap({
            asyncComponent: () => import('./Session.svelte') as any,
            conditions: [requireLogin],
        }),
        '/recordings/:id': wrap({
            asyncComponent: () => import('./Recording.svelte') as any,
            conditions: [requireLogin],
        }),
        '/log': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            conditions: [requireLogin],
        }),
        '/log/user/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'user',
            },
            conditions: [requireLogin],
        }),
        '/log/access-role/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'access-role',
            },
            conditions: [requireLogin],
        }),
        '/log/admin-role/:id': wrap({
            asyncComponent: () => import('./Log.svelte') as any,
            props: {
                filterKind: 'admin-role',
            },
            conditions: [requireLogin],
        }),
        '/config': wrap({
            asyncComponent: () => import('./config/Config.svelte') as any,
            conditions: [requireLogin],
        }),
        '/gateway': wrap({
            asyncComponent: () => import('../gateway/App.svelte') as any,
            props: {
                routePrefix: '/gateway',
                hideAdminButton: true,
                brandHref: '/@warpgate#/gateway',
                embedded: true,
            },
        }),
    }
    routes['/config/*'] = routes['/config']!
    routes['/gateway/*'] = routes['/gateway']!
</script>

<Loadable promise={initPromise}>
    <div class="app {fullScreenRoute ? 'fullscreen' : 'container-lg'}">
        {#if !fullScreenRoute}
        <header>
            <a href="/@warpgate" class="d-flex logo-link me-4">
                <Brand />
            </a>
            {#if $serverInfo?.username}
                <a use:link use:active href="/">Sessions</a>
                <a use:link use:active href="/config">Config</a>
                <a use:link use:active href="/log">Log</a>
                <a use:link use:active href="/gateway">Gateway</a>
            {/if}
            <div class="ms-auto">
                <AuthBar />
            </div>
        </header>
        {/if}
        <main>
            <Router {routes}/>
        </main>

        {#if !fullScreenRoute}
        <footer>
            <span class="me-auto ms-3">
                {$serverInfo?.version}
            </span>
            <ThemeSwitcher />
        </footer>
        {/if}
    </div>
</Loadable>

<style lang="scss">
    @media (max-width: 767px) {
        .logo-link {
            display: none !important;
        }
    }

    .app {
        min-height: 100vh;
        display: flex;
        flex-direction: column;
    }

    .app.fullscreen {
        max-width: none;
        padding: 0;
    }

    header, footer {
        flex: none;
    }

    main {
        flex: 1 0 0;
    }

    header {
        display: flex;
        align-items: center;
        gap: .2rem;
        min-height: 52px;
        padding: 0;
        margin: 0 0 1rem;
        overflow-x: auto;

        > a:not(.logo-link) {
            color: var(--bs-body-color);
            font-size: .95rem;
            font-weight: 500;
            line-height: 1;
            margin-right: 0;
            padding: .45rem .65rem;
            text-decoration: none;
            white-space: nowrap;
            border-radius: var(--bs-border-radius);
        }

        > a:not(.logo-link):hover,
        > a:not(.logo-link):global(.active) {
            background: var(--bs-list-group-action-hover-bg);
            color: var(--bs-list-group-action-hover-color);
        }

        .logo-link {
            flex: 0 0 auto;
            margin-right: 1rem !important;
        }
    }

    .app:not(.fullscreen) main {
        padding-top: .25rem;
    }
</style>
