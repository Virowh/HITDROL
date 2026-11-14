# HITDROL startup artwork

Generated with the built-in image_gen tool from the user-provided icy sword/HITDROL reference on 2026-10-25. The original application logo is unchanged. Runtime animation, cracks, chips, lighting, reflection, and transition are drawn by `src/intro.rs`; these three PNGs are its source layers. `build.rs` creates smaller embedded textures without changing the source assets.

## ice-background.png

Use case: precise-object-edit. Asset: clean background plate for a 2.5 second game launcher intro. Image 1 is the edit target. Remove ALL of the sword, ALL lettering HITDROL, and ALL flying foreground shards from this image. Retain the very dark midnight-black and deep blue background and the detailed icy reflective ground plane with cool cyan cracks, low mist along the horizon. Reconstruct the vacated areas seamlessly. Wide landscape 16:10 framing, horizon near 78 percent down the canvas, central ground contact area at 80 percent down. Keep upper two thirds nearly black and quiet so an animated sword and wordmark can be composited later. No objects, no letters, no sword, no logo, no watermark. High quality cinematic clean plate. Do not depict a completed impact explosion.

## ice-sword.png

Use case: background-extraction / precise-object-edit. Image 1 is the reference and edit target. Extract ONLY the straight vertical icy sword from the reference, reconstructing the section hidden behind the lettering into a continuous elegant faceted ice blade. Preserve the diamond pommel, dark blue wrapped grip, ice wing crossguard, crystalline cyan-white-blue material and sharp downward point. Full single sword, upright hilt up tip down, perfectly centered, symmetric, large on a portrait canvas with modest padding. This is an animation sprite: genuine TRANSPARENT background, precise soft alpha at luminous edges. No text, no HITDROL lettering, no ground, no flying ice shards, no environment, no drop shadow plane, no frame, no checkerboard. The entire sword including pommel and point must fit without clipping.

## ice-wordmark.png

Use case: background-extraction / identity-preserve. Image 1 is the reference and edit target. Extract and reconstruct ONLY its wide icy crystalline wordmark HITDROL, exactly seven capital letters H I T D R O L, maintaining the sharp angular custom typography, white-cyan top faces and deep blue faceted side faces. All letters must be complete and readable, including middle D/R where the sword overlaps in the reference. A single horizontal wordmark centered on a wide canvas, close framing with comfortable edge padding. Genuine TRANSPARENT background with precise alpha. No sword, no ground, no scene, no floating debris, no second logo, no subtitle, no watermark, no checkerboard. Keep a crisp polished dimensional frozen-ice appearance, matching the reference.
