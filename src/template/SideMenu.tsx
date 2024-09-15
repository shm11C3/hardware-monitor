import { selectedMenuAtom } from "@/atom/ui";
import type { SelectedMenuType } from "@/types/ui";
import { CaretDoubleLeft, CaretDoubleRight } from "@phosphor-icons/react";
import { useAtom } from "jotai";
import { useState } from "react";
import { memo, useCallback } from "react";
import { tv } from "tailwind-variants";

const buttonClasses = tv({
  base: "fixed top-0 rounded-xl hover:bg-gray-700 p-2 transition-all",
  variants: {
    open: {
      true: "left-64",
      false: "left-0",
    },
  },
});

const sideMenuClasses = tv({
  base: "fixed top-0 left-0 h-full bg-gray-800 text-white w-64 transform transition-transform duration-300 ease-in-out",
  variants: {
    open: {
      true: "translate-x-0",
      false: "-translate-x-full",
    },
  },
});

const menuItemClasses = tv({
  base: "mb-2 rounded-lg transition-colors",
  variants: {
    selected: {
      true: "text-white font-bold",
      false: "text-slate-400 hover:text-slate-100",
    },
  },
});

const menuTitles: Record<SelectedMenuType, string> = {
  dashboard: "Dashboard",
  usage: "Usage",
  settings: "Settings",
};

const SideMenu = () => {
  const [isOpen, setIsOpen] = useState(false);
  const [selectedMenu, setSelectedMenu] = useAtom(selectedMenuAtom);

  const toggleMenu = useCallback(() => {
    setIsOpen((prev) => !prev);
  }, []);

  const handleMenuClick = useCallback(
    (type: SelectedMenuType) => {
      setSelectedMenu(type);
    },
    [setSelectedMenu],
  );

  const MenuItem = memo(({ type }: { type: SelectedMenuType }) => {
    return (
      <li className={menuItemClasses({ selected: selectedMenu === type })}>
        <button
          type="button"
          className="p-2 w-full h-full text-left"
          onClick={() => handleMenuClick(type)}
          aria-expanded={isOpen}
          aria-label={isOpen ? "Close menu" : "Open menu"}
        >
          {menuTitles[type]}
        </button>
      </li>
    );
  });

  return (
    <div>
      <button
        type="button"
        className={buttonClasses({ open: isOpen })}
        onClick={toggleMenu}
      >
        {isOpen ? <CaretDoubleLeft /> : <CaretDoubleRight />}
      </button>
      <div className={sideMenuClasses({ open: isOpen })}>
        <ul className="p-4">
          <li className="mb-4">
            <h2 className="text-xl font-bold">Hardware Monitor</h2>
          </li>
          <MenuItem type="dashboard" />
          <MenuItem type="usage" />
          <MenuItem type="settings" />
        </ul>
      </div>
    </div>
  );
};

export default SideMenu;
