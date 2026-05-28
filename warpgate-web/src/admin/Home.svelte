<script lang="ts">
    import Fa from 'svelte-fa'
    import { faCircleDot as iconActive } from '@fortawesome/free-regular-svg-icons'
    import { onDestroy } from 'svelte'
    import { link } from 'svelte-spa-router'
    import { api, type SessionSnapshot } from 'admin/lib/api'
    import { formatDistance } from 'date-fns'
    import { timer, Observable, switchMap, from, combineLatest, merge, map, share } from 'rxjs'
    import RelativeDate from './RelativeDate.svelte'
    import AsyncButton from 'common/AsyncButton.svelte'
    import ItemList, { type LoadOptions, type PaginatedResponse } from 'common/ItemList.svelte'
    import { Input } from '@sveltestrap/sveltestrap'
    import { autosave } from 'common/autosave'
    import GettingStarted from 'common/GettingStarted.svelte'
    import { serverInfo } from 'gateway/lib/store'
    import { adminPermissions } from './lib/store'
    import PermissionGate from './lib/PermissionGate.svelte'

    let [showActiveOnly, showActiveOnly$] = autosave('sessions-list:show-active-only', false)
    let [showLoggedInOnly, showLoggedInOnly$] = autosave('sessions-list:show-logged-in-only', true)

    let activeSessionCount: number|undefined = $state()

    function createSessionChangesStream (): Observable<Event> {
        return new Observable<Event>(subscriber => {
            let socket: WebSocket|undefined
            let reconnectTimer: number|undefined
            let stopped = false

            const connect = () => {
                if (stopped) {
                    return
                }

                const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
                const currentSocket = new WebSocket(`${scheme}://${location.host}/@warpgate/admin/api/sessions/changes`)
                socket = currentSocket
                currentSocket.addEventListener('open', event => subscriber.next(event))
                currentSocket.addEventListener('message', event => subscriber.next(event))
                currentSocket.addEventListener('error', () => currentSocket.close())
                currentSocket.addEventListener('close', () => {
                    if (socket === currentSocket) {
                        socket = undefined
                    }
                    if (!stopped) {
                        reconnectTimer = window.setTimeout(connect, 1000)
                    }
                })
            }

            connect()

            return () => {
                stopped = true
                if (reconnectTimer !== undefined) {
                    window.clearTimeout(reconnectTimer)
                }
                socket?.close()
            }
        })
    }

    let sessionChanges$ = createSessionChangesStream().pipe(share())

    function loadSessions (opt: LoadOptions): Observable<PaginatedResponse<SessionSnapshot>> {
        if (!$adminPermissions.sessionsView) {
            // return empty observable
            return from(Promise.resolve({ items: [], offset: 0, total: 0 }))
        }
        return combineLatest([
            showActiveOnly$,
            showLoggedInOnly$,
            merge(timer(0, 60000), sessionChanges$),
        ]).pipe(switchMap(([activeOnly, loggedInOnly]) => {
            return from(Promise.all([
                api.getSessions({
                    activeOnly: true,
                    limit: 1,
                }),
                api.getSessions({
                    activeOnly,
                    loggedInOnly,
                    ...opt,
                }),
            ])).pipe(map(([activeSessions, sessions]) => {
                activeSessionCount = activeSessions.total
                return sessions
            }))
        }))
    }

    async function _reloadSessions (): Promise<void> {
        activeSessionCount = (await api.getSessions({ activeOnly: true })).total
    }

    async function closeAllSesssions () {
        await api.closeAllSessions()
    }

    function describeSession (session: SessionSnapshot): string {
        let user = session.username ?? (session.ended ? '<not logged in>' : '<logging in>')
        if (!session.target) {
            return user
        }
        let target = session.target.name
        return `${user} on ${target}`
    }

    _reloadSessions()
    const interval = setInterval(_reloadSessions, 1000000)
    onDestroy(() => clearInterval(interval))
</script>

{#if $serverInfo?.setupState}
    <GettingStarted
        setupState={$serverInfo?.setupState} />
{/if}

<PermissionGate perm="sessionsView" message="You have no permission to view sessions.">
    {#if activeSessionCount !== undefined}
    <div class="page-summary-bar">
        {#if activeSessionCount }
            <h1>
                <span>active sessions:</span> <span class="counter">{activeSessionCount}</span>
            </h1>
            <div class="ms-auto">
                {#if $adminPermissions.sessionsTerminate}
                <AsyncButton color="warning" click={closeAllSesssions}>
                    Close all
                </AsyncButton>
                {/if}
            </div>
        {:else}
            <h1>no active sessions</h1>
        {/if}
    </div>
    {/if}

    <ItemList load={loadSessions} pageSize={100}>
        {#snippet header()}
            <div  class="d-flex align-items-center mb-1 w-100">
                <div class="ms-auto"></div>
                <Input class="ms-3" type="switch" label="Active only" bind:checked={$showActiveOnly} />
                <Input class="ms-3" type="switch" label="Logged in only" bind:checked={$showLoggedInOnly} />
            </div>
        {/snippet}

        {#snippet item(session)}
            <a

                class="list-group-item list-group-item-action"
                href="/sessions/{session.id}"
                use:link>
                <div class="main">
                    <div class="icon" class:text-success={!session.ended}>
                        {#if !session.ended}
                            <Fa icon={iconActive} fw />
                        {/if}
                    </div>
                    <div class="protocol text-muted me-2">{session.protocol}</div>
                    <strong>
                        {describeSession(session)}
                    </strong>

                    <div class="meta">
                        {#if session.ended }
                            {formatDistance(new Date(session.started), new Date(session.ended))}
                        {/if}
                    </div>

                    <div class="meta ms-auto">
                        <RelativeDate date={session.started} />
                    </div>
                </div>
            </a>
        {/snippet}
    </ItemList>
</PermissionGate>

<style lang="scss">
    .list-group-item {
        .icon {
            display: flex;
            align-items: center;
            margin-right: 5px;
            width: 20px;
        }

        .main {
            display: flex;
            align-items: center;
            gap: .5rem;
            min-width: 0;
        }

        .protocol {
            min-width: 3.5rem;
            font-size: .78rem;
            font-weight: 600;
            letter-spacing: .02rem;
            text-transform: uppercase;
        }

        .meta {
            opacity: .75;
            margin-left: 1rem;
            font-size: .75rem;
            white-space: nowrap;
        }

        strong {
            min-width: 0;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }
    }

    @media (max-width: 576px) {
        .list-group-item .main {
            align-items: flex-start;
            flex-wrap: wrap;
        }

        .list-group-item .meta {
            margin-left: 0;
        }
    }
</style>
