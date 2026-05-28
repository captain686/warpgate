<script lang="ts">
    import { faCircle } from '@fortawesome/free-regular-svg-icons'
    import { faCircleCheck, faExternalLink } from '@fortawesome/free-solid-svg-icons'
    import { Button, ListGroup } from '@sveltestrap/sveltestrap'
    import type { SetupState } from 'gateway/lib/api'
    import Fa from 'svelte-fa'

    interface Props {
        setupState: SetupState
    }

    let { setupState }: Props = $props()

    const dismissedStorageKey = 'warpgate:getting-started-dismissed'
    let dismissed = $state(localStorage.getItem(dismissedStorageKey) === 'true')

    function dismiss () {
        dismissed = true
        localStorage.setItem(dismissedStorageKey, 'true')
    }
</script>

{#if !dismissed}
<div class="getting-started-help border-secondary">
    <div class="heading">
        <h2>getting started</h2>
        <Button
            aria-label="Hide getting started"
            class="dismiss"
            color="link"
            size="sm"
            type="button"
            on:click={dismiss}
        >
            <span aria-hidden="true">&times;</span>
        </Button>
    </div>

    <ListGroup flush>
        <!-- eslint-disable-next-line svelte/no-target-blank -->
        <a href="https://warpgate.null.page/docs/" target="_blank" class="list-group-item list-group-item-action d-flex align-items-center">
            <Fa icon={faCircle} />
            <div class="item-text me-auto">
                <div>Check out the documentation</div>
            </div>
            <Fa icon={faExternalLink} />
        </a>

        <a href="/@warpgate#/config/targets/create" class="list-group-item list-group-item-action d-flex align-items-center">
            <Fa icon={setupState.hasTargets ? faCircleCheck : faCircle} />
            <div class="item-text">
                <div>Add a target</div>
                <small>Targets are the servers and services that your users will connect to through Warpgate</small>
            </div>
        </a>

        <a href="/@warpgate#/config/users/create" class="list-group-item list-group-item-action d-flex align-items-center">
            <Fa icon={setupState.hasUsers ? faCircleCheck : faCircle} />
            <div class="item-text">
                <div>Add a non-admin user</div>
                <small>Create separate non-admin user accounts for your users</small>
            </div>
        </a>
    </ListGroup>
</div>
{/if}


<style lang="scss">
    .getting-started-help {
        margin-bottom: 1.5rem;
        border-top: 1px solid var(--bs-border-color);
        border-bottom: 1px solid var(--bs-border-color);
        padding: 1rem 0;

        .heading {
            display: flex;
            align-items: center;
            gap: .75rem;
            margin-bottom: .65rem;
        }

        h2 {
            font-family: inherit;
            font-size: 1.05rem;
            font-weight: 650;
            margin: 0;
        }

        .heading :global(.dismiss) {
            margin-left: auto;
            color: var(--bs-secondary-color);
            font-size: 1.5rem;
            line-height: 1;
            text-decoration: none;
        }

        .item-text {
            min-width: 0;
            margin-left: .75rem;

            small {
                color: var(--bs-secondary-color);
                line-height: 1.3;
            }
        }
    }
</style>
