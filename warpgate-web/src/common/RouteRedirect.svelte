<script lang="ts">
    import { onMount } from 'svelte'
    import { replace } from 'svelte-spa-router'

    interface Props {
        to: string
    }

    let { to }: Props = $props()

    onMount(() => {
        void replace(to)
    })
</script>
