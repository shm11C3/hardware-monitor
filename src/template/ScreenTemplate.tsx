interface ScreenTemplateProps {
  title?: string;
  children: React.ReactNode;
}

const ScreenTemplate: React.FC<ScreenTemplateProps> = ({ title, children }) => {
  return (
    <div className="w-3/4 mx-auto px-4 pt-12">
      {title && <h2 className="text-3xl font-bold py-3"> {title}</h2>}
      {children}
    </div>
  );
};

export default ScreenTemplate;
