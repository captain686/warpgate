<script lang="ts">
    import { faArrowRight } from '@fortawesome/free-solid-svg-icons'
    import Fa from 'svelte-fa'
    import { link } from 'svelte-spa-router'
    import active from 'svelte-spa-router/active'
    import { classnames } from './sveltestrap-s5-ports/_sveltestrapUtils'
    import type { Snippet } from 'svelte'

    interface Props {
        class?: string,
        title?: string,
        titleSnippet?: Snippet<[]>,
        description?: string,
        descriptionSnippet?: Snippet<[]>,
        addonSnippet?: Snippet<[]>,
        href: string,
        small?: boolean,
    }

    let {
        title,
        titleSnippet,
        'class': className,
        description,
        descriptionSnippet,
        addonSnippet,
        href,
        small,
    }: Props = $props()

    let classes = $derived(classnames(
        className,
        'link',
        small ? 'sm' : false,
    ))
</script>

<a
    class={classes}
    href={href}
    use:link
    use:active
>
    <div class="text">
        <div class="title">
            {#if titleSnippet}
                {@render titleSnippet()}
            {:else}
                {title}
            {/if}
        </div>
        <div class="description text-muted">
            {#if descriptionSnippet}
                {@render descriptionSnippet()}
            {:else if description}
                {description}
            {/if}
        </div>
    </div>
    {@render addonSnippet?.()}
    <div class="icon">
        <Fa class="icon" icon={faArrowRight} />
    </div>
</a>


<style lang="scss">
    a {
        cursor: pointer;
        display: flex;
        width: 100%;
        text-decoration: none;
        padding: .65rem .875rem;
        border-radius: var(--bs-border-radius);
        align-items: center;
        gap: .75rem;

        .text {
            flex-grow: 1;
            min-width: 0;
        }

        &:hover, &.active {
            background: var(--bs-list-group-action-hover-bg);
            .title {
                color: var(--bs-list-group-action-hover-color);
            }
        }

        &:active {
            background: var(--bs-list-group-action-active-bg);
            .title {
                color: var(--bs-list-group-action-active-color);
            }
        }

        .title {
            margin-bottom: .15rem;
            font-size: 1rem;
            font-weight: 600;
            line-height: 1.25;
            text-decoration: none;
        }

        .icon {
            flex: 0 0 auto;
            color: var(--bs-secondary-color);
            font-size: .85rem;
            opacity: .75;
        }

        &.link:hover .icon,
        &.active .icon {
            opacity: 1;
        }

        .description {
            text-decoration: none;
            line-height: 1.25;
            font-size: .85rem;
        }

        &.sm {
            padding: .45rem .7rem;

            .title {
                font-size: .95rem;
            }

            .description {
                font-size: .8rem;
            }

            .icon {
                display: none;
            }
        }
    }

</style>
