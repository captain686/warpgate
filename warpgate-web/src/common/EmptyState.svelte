<script lang="ts">
    export let title: string
    export let hint = ''
</script>

<div class="empty-state">
    <h2>{title}</h2>
    {#if hint}
        <p class="text-muted">{hint}</p>
    {/if}
</div>

<style lang="scss">
    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        width: 100%;
        max-width: 34rem;
        text-align: center;
        margin: 2.5rem auto;
        padding: 1rem 0;
        color: var(--bs-secondary-color);
    }

    h2 {
        font-family: inherit;
        font-size: 1.05rem;
        font-weight: 650;
        margin-bottom: .35rem;
    }

    p {
        font-size: .88rem;
        margin-bottom: 0;
    }
</style>
