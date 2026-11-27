"""Regression checks for catalog definitions and cross-rig seed inputs."""
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import seeds


class CatalogSeeds(unittest.TestCase):
    def setUp(self):
        self.neutral = b'"neutral_items" { "neutral_tiers" { "1" {} } }'
        sources = {
            "resource/localization/abilities_english.txt": (
                b'"DOTA_Tooltip_ability_item_boots_Description" "Move faster"\n'
                b'"DOTA_Tooltip_ability_item_boots_Lore" "Old boots"\n'
                b'"DOTA_Tooltip_ability_axe_battle_hunger" "Battle Hunger"\n'
                b'"DOTA_Tooltip_Facet_axe_call_out" "Call Out"\n'
                b'"DOTA_Tooltip_modifier_axe_battle_hunger" "Hungering"\n'
            ),
            "resource/localization/dota_english.txt": b'"DOTA_Tooltip_Cost" "Cost"\n',
            "scripts/npc/items.txt": b'''"DOTAAbilities"
{
"item_boots"
{
"ItemCost" "500"
}
"item_recipe_boots"
{
"ItemRecipe" "1"
"ItemResult" "item_boots"
"ItemRequirements" { "01" "item_slippers" }
}
"item_neutral_only"
{
"ItemCost" "0"
}
}
''',
            "scripts/npc/npc_abilities.txt": b'"ability_base"\n{\n"AbilityCooldown" "0"\n}\n',
            "scripts/npc/heroes/npc_dota_hero_axe.txt": b'''"DOTAHeroes"
{
"npc_dota_hero_axe"
{
"Ability1" "axe_battle_hunger"
"Facets" { "axe_call_out" { "Icon" "armor" } }
"AbilityDefinitions"
{
"axe_battle_hunger"
{
"AbilityValues" { "damage" "10 20 30 40" }
}
}
}
}
''',
            "scripts/npc/neutral_items.txt": self.neutral,
            "scripts/shops.txt": b'"dota_shops" { "consumables" { "item" "item_boots" } }',
        }
        self.written = {}
        with patch.object(seeds, "write", side_effect=self.capture):
            seeds.texts(SimpleNamespace(read=sources.get))

    def capture(self, target, name, data):
        self.written[target] = data

    def parts(self, target):
        return [
            chunk.split(b"\n", 1)[1]
            for chunk in self.written[target].split(seeds.SEPARATOR)
        ]

    def test_suffixed_localization_keeps_shop_result_and_recipe_definitions(self):
        shops, items, neutral = self.parts("shops_catalog")
        self.assertEqual(shops, self.written["shop"])
        self.assertTrue(items.startswith(b'"DOTAAbilities"\n{'))
        self.assertIn(b'"item_boots"\n{', items)
        self.assertIn(b'"item_recipe_boots"\n{', items)
        self.assertNotIn(b'"item_neutral_only"', items)
        self.assertEqual(neutral, self.neutral)

    def test_tooltips_include_hero_definitions_values_facets_and_matching_localization(self):
        abilities, dota, items, global_abilities, hero = self.parts("tooltip_catalog")
        self.assertIn(b'"DOTA_Tooltip_ability_item_boots_Description"', abilities)
        self.assertIn(b'"DOTA_Tooltip_ability_item_boots_Lore"', abilities)
        self.assertIn(b'"DOTA_Tooltip_ability_axe_battle_hunger"', abilities)
        self.assertIn(b'"DOTA_Tooltip_Facet_axe_call_out"', abilities)
        self.assertIn(b'"DOTA_Tooltip_modifier_axe_battle_hunger"', abilities)
        self.assertIn(b'"DOTA_Tooltip_Cost" "Cost"', dota)
        self.assertEqual(items, self.parts("shops_catalog")[1])
        self.assertIn(b'"axe_battle_hunger"\n{', global_abilities)
        self.assertIn(b'"AbilityValues"', global_abilities)
        self.assertIn(b'"Facets"', hero)

    def test_excerpt_budget_counts_utf8_bytes_and_keeps_whole_blocks(self):
        first = '"first"\n{\n"text" "éé"\n}\n'
        second = '"second"\n{\n"text" "tail"\n}\n'
        read = seeds.excerpt(first + second, ["first", "second"], len(first.encode()))
        self.assertEqual(read, first)


class RigSeeds(unittest.TestCase):
    def test_cross_rig_seed_contains_base_hero_then_wearable(self):
        sources = {
            "models/heroes/axe/axe.vmdl_c": b"base rig",
            "models/items/axe/axe_belt001/axe_belt001.vmdl_c": b"wearable rig",
        }
        with patch.object(seeds, "write") as write:
            seeds.rig_pair(SimpleNamespace(read=sources.get))
        target, name, data = write.call_args.args
        self.assertEqual(target, "model_rig")
        self.assertEqual(data, seeds.bundle(list(sources.items())))


if __name__ == "__main__":
    unittest.main()
