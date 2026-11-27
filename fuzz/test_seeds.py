"""Regression checks for the catalog harnesses' multipart contracts."""
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import seeds


class CatalogSeeds(unittest.TestCase):
    def setUp(self):
        self.items = '"item_boots"\n{\n"ItemCost" "500"\n}\n'
        self.neutral = b'"neutral_items" { "neutral_tiers" { "1" {} } }'
        sources = {
            "resource/localization/abilities_english.txt":
                b'"DOTA_Tooltip_ability_item_boots" "Boots"\n',
            "resource/localization/dota_english.txt": b'"DOTA_Tooltip_Cost" "Cost"\n',
            "scripts/npc/items.txt": self.items.encode(),
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

    def test_shops_get_layout_rooted_definitions_and_neutral_tiers(self):
        shops, items, neutral = self.parts("shops_catalog")
        self.assertEqual(shops, self.written["shop"])
        self.assertEqual(items, ('"DOTAAbilities"\n{\n' + self.items + '\n}\n').encode())
        self.assertEqual(neutral, self.neutral)

    def test_tooltips_keep_localization_slots_before_definition_scripts(self):
        abilities, dota, items, global_abilities = self.parts("tooltip_catalog")
        self.assertIn(b'"DOTA_Tooltip_ability_item_boots" "Boots"', abilities)
        self.assertIn(b'"DOTA_Tooltip_Cost" "Cost"', dota)
        self.assertEqual(items, self.parts("shops_catalog")[1])
        self.assertEqual(global_abilities, b'"DOTAAbilities"\n{\n\n}\n')


if __name__ == "__main__":
    unittest.main()
