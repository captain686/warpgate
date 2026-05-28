<script lang="ts">
    import type { Snippet } from 'svelte'

    interface Props {
        id: string
        title: string
        bodyTitle?: string
        class?: string
        hideHeading?: boolean
        children?: Snippet
    }

    const props: Props = $props()
</script>

<section data-section={props.id} data-section-title={props.title} class="sectioned-form-section {props.class ?? ''}" id={`${props.id}-heading`}>
    {#if !props.hideHeading}
        <h4>{props.bodyTitle ?? props.title}</h4>
    {/if}
    {@render props.children?.()}
</section>

<style>
    section {
        scroll-margin-top: 50px;
    }

    .sectioned-form-section {
        padding: 0 0 1.25rem;
        margin-bottom: 1.25rem;
        border-bottom: 1px solid var(--bs-border-color);
    }

    .sectioned-form-section:last-child {
        border-bottom: none;
        margin-bottom: 0;
    }

    .sectioned-form-section h4 {
        margin-top: 0;
        margin-bottom: .85rem;
        color: var(--bs-body-color);
        font-size: 1.05rem;
        font-weight: 650;
    }
</style>
