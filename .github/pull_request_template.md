<!-- Titre : résumé Conventional Commits, ex. « feat(workflows): ajoute le déclencheur Retour ». Il devient le message du commit (squash) et alimente le changelog. -->

## Résumé

-

## Captures

<!-- Changement visible : avant / après. Supprimer sinon. -->

## Vérifications

- [ ] `cargo fmt --check` et `cargo clippy --all-targets -- -D warnings` propres
- [ ] `cargo test` vert (Windows et Linux en CI)
- [ ] Testé en vrai sur le système concerné (caméra, raccourcis, tray…)
- [ ] Changement cassant signalé (`!` ou pied `BREAKING CHANGE:`)
