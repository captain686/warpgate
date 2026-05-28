<script lang="ts">
    import type { Snippet } from 'svelte'

    interface Props {
        class?: string
        start?: Snippet
        end?: Snippet
        children?: Snippet
    }

    const props: Props = $props()
</script>

<div class="sticky-action-bar {props.class ?? ''}">
    <div class="sticky-action-bar-start">
        {@render props.start?.()}
    </div>
    <div class="sticky-action-bar-content">
        {@render props.children?.()}
    </div>
    <div class="sticky-action-bar-end">
        {@render props.end?.()}
    </div>
</div>

<style>
    .sticky-action-bar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.5rem;
        min-height: 3.5rem;
        padding: .75rem 0;
        border-top: 1px solid var(--bs-border-color);
        background: var(--bs-body-bg);
        position: sticky;
        bottom: 0;
        z-index: 5;
        box-shadow: 0 -10px 24px rgba(0, 0, 0, .08);
    }

    .sticky-action-bar-start,
    .sticky-action-bar-content,
    .sticky-action-bar-end {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .sticky-action-bar-start {
        margin-right: auto;
    }

    @media (max-width: 576px) {
        .sticky-action-bar,
        .sticky-action-bar-start,
        .sticky-action-bar-content,
        .sticky-action-bar-end {
            width: 100%;
            align-items: stretch;
        }

        .sticky-action-bar-content,
        .sticky-action-bar-end {
            justify-content: flex-end;
        }
    }
</style>
